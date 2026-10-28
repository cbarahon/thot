//! Purpose: Safely extracts project archives and snapshots project directories into staging.
use flate2::read::GzDecoder;
use std::{
    collections::HashSet,
    fs::{self, File, OpenOptions},
    io,
    path::{Component, Path, PathBuf},
};

const MAX_ARCHIVE_BYTES: u64 = 100 * 1024 * 1024;
const MAX_EXPANDED_BYTES: u64 = 500 * 1024 * 1024;
const MAX_ENTRIES: usize = 2_000;

pub fn snapshot_project_directory(
    source: &Path,
    destination: &Path,
    excluded_paths: &[PathBuf],
) -> Result<PathBuf, String> {
    let source_metadata = fs::symlink_metadata(source)
        .map_err(|e| format!("cannot inspect project directory: {e}"))?;
    if source_metadata.file_type().is_symlink() || !source_metadata.is_dir() {
        return Err("project source must be a real directory".into());
    }
    let source = source
        .canonicalize()
        .map_err(|e| format!("cannot resolve project directory: {e}"))?;
    if destination.exists() {
        return Err("project snapshot destination already exists".into());
    }
    let destination_name = destination
        .file_name()
        .ok_or("project snapshot destination must have a final path component")?;
    let destination_parent = destination
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    fs::create_dir_all(destination_parent)
        .map_err(|e| format!("cannot create project snapshot parent: {e}"))?;
    let destination_parent = destination_parent
        .canonicalize()
        .map_err(|e| format!("cannot resolve project snapshot parent: {e}"))?;
    if destination_parent == source || destination_parent.starts_with(&source) {
        return Err("project snapshot destination must be outside the source directory".into());
    }
    let destination = destination_parent.join(destination_name);
    if destination.starts_with(&source) {
        return Err("project snapshot destination must be outside the source directory".into());
    }
    fs::create_dir(&destination)
        .map_err(|e| format!("cannot create project snapshot directory: {e}"))?;

    let mut entry_count = 0;
    let mut expanded_bytes = 0;
    let result = copy_project_tree(
        &source,
        &destination,
        Path::new(""),
        excluded_paths,
        &mut entry_count,
        &mut expanded_bytes,
    );
    if let Err(error) = result {
        let _ = fs::remove_dir_all(&destination);
        return Err(error);
    }
    Ok(destination)
}

fn copy_project_tree(
    source_root: &Path,
    destination_root: &Path,
    relative_directory: &Path,
    excluded_paths: &[PathBuf],
    entry_count: &mut usize,
    expanded_bytes: &mut u64,
) -> Result<(), String> {
    let source_directory = source_root.join(relative_directory);
    for entry in fs::read_dir(&source_directory).map_err(|e| {
        format!(
            "cannot read project directory {}: {e}",
            source_directory.display()
        )
    })? {
        let entry = entry.map_err(|e| format!("cannot read project entry: {e}"))?;
        let source_path = entry.path();
        let relative_path = source_path
            .strip_prefix(source_root)
            .map_err(|_| "project entry is outside the source directory")?;
        if excluded_paths.iter().any(|excluded| {
            source_path.as_path() == excluded.as_path() || source_path.starts_with(excluded)
        }) {
            continue;
        }
        if relative_directory.as_os_str().is_empty()
            && (relative_path == Path::new("vms")
                || relative_path == Path::new("test_records")
                || is_project_archive_name(&entry.file_name()))
        {
            continue;
        }

        *entry_count += 1;
        if *entry_count > MAX_ENTRIES {
            return Err("project directory contains more than 2,000 entries".into());
        }
        let metadata = fs::symlink_metadata(&source_path)
            .map_err(|e| format!("cannot inspect project entry: {e}"))?;
        if metadata.file_type().is_symlink() {
            return Err(format!(
                "project symlinks are not allowed: {}",
                relative_path.display()
            ));
        }
        let destination_path = destination_root.join(relative_path);
        if !destination_path.starts_with(destination_root) {
            return Err("project entry escapes snapshot directory".into());
        }
        if metadata.is_dir() {
            fs::create_dir(&destination_path)
                .map_err(|e| format!("cannot create snapshot directory: {e}"))?;
            copy_project_tree(
                source_root,
                destination_root,
                relative_path,
                excluded_paths,
                entry_count,
                expanded_bytes,
            )?;
            fs::set_permissions(&destination_path, metadata.permissions())
                .map_err(|e| format!("cannot preserve directory permissions: {e}"))?;
        } else if metadata.is_file() {
            *expanded_bytes = (*expanded_bytes).saturating_add(metadata.len());
            if *expanded_bytes > MAX_EXPANDED_BYTES {
                return Err("expanded project directory exceeds 500 MiB".into());
            }
            fs::copy(&source_path, &destination_path)
                .map_err(|e| format!("cannot copy project file: {e}"))?;
            fs::set_permissions(&destination_path, metadata.permissions())
                .map_err(|e| format!("cannot preserve file permissions: {e}"))?;
        } else {
            return Err(format!(
                "project special files are not allowed: {}",
                relative_path.display()
            ));
        }
    }
    Ok(())
}

fn is_project_archive_name(name: &std::ffi::OsStr) -> bool {
    let name = name.to_string_lossy().to_ascii_lowercase();
    [".thot.gz", ".thot.tgz", ".thot.zip"]
        .iter()
        .any(|suffix| name.ends_with(suffix))
}

pub fn extract_project_archive(archive: &Path, destination: &Path) -> Result<PathBuf, String> {
    let metadata = fs::symlink_metadata(archive)
        .map_err(|e| format!("cannot inspect project archive: {e}"))?;
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        return Err("project archive must be a regular file".into());
    }
    if metadata.len() > MAX_ARCHIVE_BYTES {
        return Err("project archive exceeds the 100 MiB limit".into());
    }
    if destination.exists() {
        return Err("project extraction destination already exists".into());
    }
    fs::create_dir_all(destination)
        .map_err(|e| format!("cannot create project staging directory: {e}"))?;
    let result = if archive
        .file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| name.ends_with(".zip"))
    {
        extract_zip(archive, destination)
    } else if archive
        .file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| name.ends_with(".tgz") || name.ends_with(".thot.gz"))
    {
        extract_tgz(archive, destination)
    } else {
        Err("project archive extension must be .thot.zip, .thot.tgz, or .thot.gz".into())
    };
    if let Err(error) = result {
        let _ = fs::remove_dir_all(destination);
        return Err(error);
    }
    let config = destination.join("configuration.yaml");
    let config_meta = fs::symlink_metadata(&config)
        .map_err(|_| "archive root must contain configuration.yaml")?;
    if config_meta.file_type().is_symlink() || !config_meta.is_file() {
        let _ = fs::remove_dir_all(destination);
        return Err("archive root configuration.yaml must be a regular file".into());
    }
    Ok(destination.to_path_buf())
}

fn safe_relative(name: &Path) -> Result<PathBuf, String> {
    if name.as_os_str().is_empty() || name.is_absolute() {
        return Err("archive entry has an empty or absolute path".into());
    }
    let mut clean = PathBuf::new();
    for component in name.components() {
        match component {
            Component::Normal(value) => clean.push(value),
            Component::CurDir => {}
            Component::ParentDir | Component::RootDir | Component::Prefix(_) => {
                return Err(format!("unsafe archive entry path: {}", name.display()));
            }
        }
    }
    if clean.as_os_str().is_empty() {
        return Ok(PathBuf::from("."));
    }
    Ok(clean)
}

fn create_entry(
    root: &Path,
    relative: &Path,
    is_dir: bool,
    executable: bool,
) -> Result<Option<File>, String> {
    let path = root.join(relative);
    if !path.starts_with(root) {
        return Err("archive entry escapes staging directory".into());
    }
    if is_dir {
        fs::create_dir_all(&path).map_err(|e| format!("cannot create archive directory: {e}"))?;
        return Ok(None);
    }
    let parent = path
        .parent()
        .ok_or("archive file has no parent directory")?;
    fs::create_dir_all(parent)
        .map_err(|e| format!("cannot create archive parent directory: {e}"))?;
    let file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&path)
        .map_err(|e| {
            format!(
                "duplicate archive path or cannot create file {}: {e}",
                path.display()
            )
        })?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = if executable { 0o750 } else { 0o640 };
        fs::set_permissions(&path, fs::Permissions::from_mode(mode))
            .map_err(|e| format!("cannot set project file permissions: {e}"))?;
    }
    Ok(Some(file))
}

fn extract_zip(archive_path: &Path, destination: &Path) -> Result<(), String> {
    let file = File::open(archive_path).map_err(|e| format!("cannot open ZIP project: {e}"))?;
    let mut archive =
        zip::ZipArchive::new(file).map_err(|e| format!("invalid ZIP project: {e}"))?;
    if archive.len() > MAX_ENTRIES {
        return Err("project archive contains more than 2,000 entries".into());
    }
    let mut seen = HashSet::new();
    let mut expanded = 0u64;
    for index in 0..archive.len() {
        let mut entry = archive
            .by_index(index)
            .map_err(|e| format!("invalid ZIP entry: {e}"))?;
        let relative = safe_relative(Path::new(entry.name()))?;
        if !seen.insert(relative.clone()) {
            return Err(format!("duplicate archive path: {}", relative.display()));
        }
        if entry
            .unix_mode()
            .is_some_and(|mode| mode & 0o170000 == 0o120000)
        {
            return Err(format!(
                "ZIP symlinks are not allowed: {}",
                relative.display()
            ));
        }
        let is_dir = entry.is_dir();
        expanded = expanded.saturating_add(entry.size());
        if expanded > MAX_EXPANDED_BYTES {
            return Err("expanded project archive exceeds 500 MiB".into());
        }
        let executable = entry.unix_mode().is_some_and(|mode| mode & 0o111 != 0);
        if let Some(mut output) = create_entry(destination, &relative, is_dir, executable)? {
            io::copy(&mut entry, &mut output)
                .map_err(|e| format!("cannot extract ZIP entry: {e}"))?;
        }
    }
    Ok(())
}

fn extract_tgz(archive_path: &Path, destination: &Path) -> Result<(), String> {
    let file = File::open(archive_path).map_err(|e| format!("cannot open tgz project: {e}"))?;
    let decoder = GzDecoder::new(file);
    let mut archive = tar::Archive::new(decoder);
    let entries = archive
        .entries()
        .map_err(|e| format!("invalid tar project: {e}"))?;
    let mut seen = HashSet::new();
    let mut expanded = 0u64;
    for (index, entry) in entries.enumerate() {
        if index >= MAX_ENTRIES {
            return Err("project archive contains more than 2,000 entries".into());
        }
        let mut entry = entry.map_err(|e| format!("invalid tar entry: {e}"))?;
        let relative = safe_relative(&entry.path().map_err(|e| format!("invalid tar path: {e}"))?)?;
        if !seen.insert(relative.clone()) {
            return Err(format!("duplicate archive path: {}", relative.display()));
        }
        let kind = entry.header().entry_type();
        if kind.is_dir() {
            create_entry(destination, &relative, true, false)?;
        } else if kind.is_file() {
            expanded = expanded.saturating_add(entry.size());
            if expanded > MAX_EXPANDED_BYTES {
                return Err("expanded project archive exceeds 500 MiB".into());
            }
            let executable = entry.header().mode().unwrap_or_default() & 0o111 != 0;
            if let Some(mut output) = create_entry(destination, &relative, false, executable)? {
                io::copy(&mut entry, &mut output)
                    .map_err(|e| format!("cannot extract tar entry: {e}"))?;
            }
        } else {
            return Err(format!(
                "tar links and special entries are not allowed: {}",
                relative.display()
            ));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use flate2::{Compression, write::GzEncoder};
    use std::io::{Cursor, Write};
    use tar::{Builder, Header};
    use zip::{ZipWriter, write::FileOptions};

    fn temp_path(label: &str) -> PathBuf {
        std::env::temp_dir().join(format!("thot-{label}-{}", uuid::Uuid::new_v4()))
    }

    #[test]
    fn extracts_root_configuration_from_zip() {
        let root = temp_path("project");
        fs::create_dir_all(&root).unwrap();
        let archive = root.join("project.thot.zip");
        let destination = root.join("staging");
        let file = File::create(&archive).unwrap();
        let mut writer = ZipWriter::new(file);
        writer
            .start_file("configuration.yaml", FileOptions::default())
            .unwrap();
        writer.write_all(b"activity_name: Demo\n").unwrap();
        writer.finish().unwrap();
        let extracted = snapshot_project_directory(&root, &destination, &[]).unwrap();
        assert_eq!(
            fs::read(extracted.join("configuration.yaml")).unwrap(),
            b"activity_name: Demo\n"
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn rejects_zip_path_traversal() {
        let root = temp_path("unsafe");
        fs::create_dir_all(&root).unwrap();
        let archive = root.join("unsafe.thot.zip");
        let destination = root.join("staging");
        let file = File::create(&archive).unwrap();
        let mut writer = ZipWriter::new(file);
        writer
            .start_file("../outside", FileOptions::default())
            .unwrap();
        writer.write_all(b"bad").unwrap();
        writer.finish().unwrap();
        assert!(snapshot_project_directory(&root, &destination, &[]).is_err());
        assert!(!destination.exists());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn extracts_root_configuration_from_tgz_with_dot_entries() {
        let root = temp_path("tgz");
        fs::create_dir_all(&root).unwrap();
        let archive = root.join("project.thot.tgz");
        let destination = root.join("staging");
        let encoder = GzEncoder::new(File::create(&archive).unwrap(), Compression::default());
        let mut builder = Builder::new(encoder);
        let mut header = Header::new_gnu();
        header.set_size(20);
        header.set_mode(0o640);
        header.set_cksum();
        builder
            .append_data(
                &mut header,
                "./configuration.yaml",
                Cursor::new(b"activity_name: Demo\n"),
            )
            .unwrap();
        let encoder = builder.into_inner().unwrap();
        encoder.finish().unwrap();
        snapshot_project_directory(&root, &destination, &[]).unwrap();
        assert_eq!(
            fs::read(destination.join("configuration.yaml")).unwrap(),
            b"activity_name: Demo\n"
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn extracts_root_configuration_from_thot_gz() {
        let root = temp_path("thot_gz");
        fs::create_dir_all(&root).unwrap();
        let archive = root.join("project.thot.gz");
        let destination = root.join("staging");
        let encoder = GzEncoder::new(File::create(&archive).unwrap(), Compression::default());
        let mut builder = Builder::new(encoder);
        let mut header = Header::new_gnu();
        header.set_size(20);
        header.set_mode(0o640);
        header.set_cksum();
        builder
            .append_data(
                &mut header,
                "configuration.yaml",
                Cursor::new(b"activity_name: Demo\n"),
            )
            .unwrap();
        let encoder = builder.into_inner().unwrap();
        encoder.finish().unwrap();
        snapshot_project_directory(&root, &destination, &[]).unwrap();
        assert_eq!(
            fs::read(destination.join("configuration.yaml")).unwrap(),
            b"activity_name: Demo\n"
        );
        fs::remove_dir_all(root).unwrap();
    }
}
