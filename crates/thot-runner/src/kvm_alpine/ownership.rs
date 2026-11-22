use std::fs::{self, File};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};
use std::os::unix::fs::FileExt;
use serde::{Deserialize, Serialize};
use serde_json::{self, json};
use thiserror::Error;

#[derive(Debug, Serialize, Deserialize)]
struct OwnedResource {
    kind: String,
    name: String,
    thot_owner_uuid: String,
    libvirt_uuid: String,
    project: String,
    storage_path: PathBuf,
    provenance_hash: String,
}

#[derive(Error, Debug)]
enum ProviderError {
    #[error("I/O error: {0}")]
    IoError(#[from] io::Error),
    #[error("Serde JSON error: {0}")]
    JsonError(#[from] serde_json::Error),
    #[error("Resource already claimed by another owner")]
    CollisionError,
    #[error("UUID mismatch")]
    UuidMismatchError,
    #[error("Missing metadata")]
    MissingMetadataError,
    #[error("Malformed JSON")]
    MalformedJsonError,
    #[error("Lock acquisition failed")]
    LockAcquisitionFailedError,
}

#[derive(Debug)]
struct OwnershipRegistry {
    root: PathBuf,
    project_locks: Mutex<std::collections::HashMap<String, File>>,
}

impl OwnershipRegistry {
    fn new(root: PathBuf) -> Result<Self, ProviderError> {
        if !root.exists() {
            fs::create_dir_all(&root)?;
        }
        Ok(Self {
            root,
            project_locks: Mutex::new(std::collections::HashMap::new()),
        })
    }

    fn claim(&self, resource: &OwnedResource) -> Result<(), ProviderError> {
        self.verify(&resource.name, &resource.libvirt_uuid)?;
        self.save_resource(resource)?;
        Ok(())
    }

    fn verify(&self, name: &str, libvirt_uuid: &str) -> Result<OwnedResource, ProviderError> {
        let path = self.resource_path(name)?;
        if !path.exists() {
            return Err(ProviderError::MissingMetadataError);
        }
        let file = File::open(&path)?;
        let resource: OwnedResource = serde_json::from_reader(&file)?;
        if resource.libvirt_uuid != libvirt_uuid {
            return Err(ProviderError::UuidMismatchError);
        }
        Ok(resource)
    }

    fn release(&self, name: &str, libvirt_uuid: &str) -> Result<(), ProviderError> {
        self.verify(name, libvirt_uuid)?;
        self.delete_resource(name)?;
        Ok(())
    }

    fn with_project_lock<T>(&self, project_id: &str, operation: impl FnOnce() -> Result<T, ProviderError>) -> Result<T, ProviderError> {
        let mut project_locks = self.project_locks.lock().unwrap();
        let lock_file_path = self.lock_file_path(project_id)?;
        let mut lock_file = File::create(&lock_file_path)?;
        lock_file.write_all(b"")?;
        unsafe {
            lock_file.set_fd_flag(libc::O_RDWR | libc::O_CREAT | libc::O_EXCL, 0)?;
        }
        if unsafe { libc::fcntl(lock_file.as_raw_fd(), libc::F_SETLKW, libc::F_WRLCK) } != 0 {
            return Err(ProviderError::LockAcquisitionFailedError);
        }
        let result = operation();
        unsafe {
            libc::fcntl(lock_file.as_raw_fd(), libc::F_SETLKW, libc::F_UNLCK)?;
        }
        project_locks.remove(project_id);
        result
    }

    fn resource_path(&self, name: &str) -> Result<PathBuf, ProviderError> {
        let sanitized_name = sanitize_name(name)?;
        Ok(self.root.join(format!("{}.json", sanitized_name)))
    }

    fn lock_file_path(&self, project_id: &str) -> Result<PathBuf, ProviderError> {
        let sanitized_project_id = sanitize_name(project_id)?;
        Ok(self.root.join(format!("{}.lock", sanitized_project_id)))
    }

    fn save_resource(&self, resource: &OwnedResource) -> Result<(), ProviderError> {
        let path = self.resource_path(&resource.name)?;
        let tmp_path = path.with_extension(format!("{}.tmp", SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs()));
        let mut file = File::create(&tmp_path)?;
        serde_json::to_writer(&mut file, resource)?;
        file.sync_all()?;
        fs::rename(&tmp_path, &path)?;
        Ok(())
    }

    fn delete_resource(&self, name: &str) -> Result<(), ProviderError> {
        let path = self.resource_path(name)?;
        fs::remove_file(&path)?;
        Ok(())
    }
}

fn sanitize_name(name: &str) -> Result<String, ProviderError> {
    let sanitized: String = name.chars().filter(|c| c.is_alphanumeric() || *c == '_').collect();
    if sanitized.is_empty() {
        return Err(ProviderError::MalformedJsonError);
    }
    Ok(sanitized)
}