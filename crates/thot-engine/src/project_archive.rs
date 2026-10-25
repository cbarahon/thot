//! Purpose: Safely extracts ZIP and TGZ project archives into a staging directory.
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
        .and_then(|n| n.to_str())
        .is_some_and(|n| n.ends_with(".zip"))
    {
        extract_zip(archive, destination)
    } else if archive
        .file_name()
        .and_then(|n| n.to_str())
        .is_some_and(|n| n.ends_with(".tgz"))
    {
        extract_tgz(archive, destination)
    } else {
        Err("project archive extension must be .thot.zip or .thot.tgz".into())
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
        let extracted = extract_project_archive(&archive, &destination).unwrap();
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
        assert!(extract_project_archive(&archive, &destination).is_err());
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
        extract_project_archive(&archive, &destination).unwrap();
        assert_eq!(
            fs::read(destination.join("configuration.yaml")).unwrap(),
            b"activity_name: Demo\n"
        );
        fs::remove_dir_all(root).unwrap();
    }
}
