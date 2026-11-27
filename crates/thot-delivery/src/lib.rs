//! Read-only intake agents for instructor-provided student deliverables.
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use chrono::{DateTime, Utc};
use reqwest::blocking::{Client, Response};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    path::{Path, PathBuf},
    process::{Command, Stdio},
    time::{Duration, SystemTime, UNIX_EPOCH},
};
use thiserror::Error;
use thot_core::sandbox_project::{DeliveryAgentConfig, DeliveryLimits};
use url::Url;

const MAX_SUPPORTED_DELIVERY_BYTES: u64 = 64 * 1024 * 1024 * 1024;
const DRIVE_READONLY_SCOPE: &str = "https://www.googleapis.com/auth/drive.readonly";

fn validate_limits(limits: &DeliveryLimits) -> Result<(), DeliveryError> {
    if limits.max_bytes == 0
        || limits.max_bytes > MAX_SUPPORTED_DELIVERY_BYTES
        || limits.max_files == 0
        || limits.max_files > 100_000
    {
        return Err(DeliveryError::Configuration(
            "delivery limits must be within the supported 64 GiB / 100,000 file ceilings".into(),
        ));
    }
    Ok(())
}

#[derive(Debug, Error)]
pub enum DeliveryError {
    #[error("delivery configuration: {0}")]
    Configuration(String),
    #[error("delivery source: {0}")]
    Source(String),
    #[error("Google Drive authorization: {0}")]
    Authorization(String),
    #[error("Google Drive request: {0}")]
    Remote(String),
    #[error("delivery I/O: {0}")]
    Io(#[from] std::io::Error),
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DeliveryInfo {
    pub name: String,
    pub size_bytes: u64,
    pub created_at: Option<String>,
    pub modified_at: Option<String>,
    #[serde(default)]
    pub provider_id: Option<String>,
    #[serde(default)]
    pub revision: Option<String>,
    #[serde(default)]
    pub mime_type: Option<String>,
    #[serde(default)]
    pub source_checksum: Option<String>,
    #[serde(default)]
    pub source_checksum_algorithm: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RetrievedDelivery {
    pub info: DeliveryInfo,
    pub sha256: String,
    pub path: PathBuf,
}

/// Both delivery backends expose the same minimal, read-only list/get contract.
pub trait DeliveryAgent {
    fn list(&mut self) -> Result<Vec<DeliveryInfo>, DeliveryError>;
    fn get(&mut self, name: &str, destination: &Path) -> Result<RetrievedDelivery, DeliveryError>;

    /// Acquire a deliverable for one evaluation. Backends may avoid a staging
    /// copy when the source is already a safe read-only input.
    fn get_for_execution(
        &mut self,
        name: &str,
        destination: &Path,
    ) -> Result<RetrievedDelivery, DeliveryError> {
        self.get(name, destination)
    }

    fn get_by_id_for_execution(
        &mut self,
        provider_id: &str,
        destination: &Path,
    ) -> Result<RetrievedDelivery, DeliveryError> {
        self.get_by_id(provider_id, destination)
    }

    /// Remove the staged copy acquired by this agent. Implementations must
    /// never delete the local source file or the remote provider object.
    fn release(&mut self, delivery: &RetrievedDelivery) -> Result<(), DeliveryError>;

    fn get_by_id(
        &mut self,
        provider_id: &str,
        destination: &Path,
    ) -> Result<RetrievedDelivery, DeliveryError> {
        let _ = (provider_id, destination);
        Err(DeliveryError::Source(
            "this delivery source does not support provider-ID selection".into(),
        ))
    }
}

pub fn build_agent(
    config: Option<&DeliveryAgentConfig>,
    project_root: &Path,
    legacy_delivery_folder: Option<&str>,
    student_input_root: Option<&str>,
    limits: &DeliveryLimits,
) -> Result<Box<dyn DeliveryAgent>, DeliveryError> {
    match config {
        Some(DeliveryAgentConfig::Gdrive { folder_url }) => {
            Ok(Box::new(GoogleDriveAgent::with_limits(folder_url, limits)?))
        }
        Some(DeliveryAgentConfig::Local { path }) => {
            let path = path
                .as_deref()
                .or(legacy_delivery_folder)
                .or(student_input_root)
                .unwrap_or("delivery");
            Ok(Box::new(LocalFilesystemAgent::with_limits(
                project_root,
                path,
                limits,
            )?))
        }
        None => {
            let path = legacy_delivery_folder
                .or(student_input_root)
                .unwrap_or("delivery");
            Ok(Box::new(LocalFilesystemAgent::with_limits(
                project_root,
                path,
                limits,
            )?))
        }
    }
}

pub fn default_student_input_root(
    config: &thot_core::sandbox_project::SandboxProject,
) -> Option<String> {
    let template = config
        .sandbox_platforms
        .iter()
        .flat_map(|platform| platform.inputs.iter())
        .find(|input| input.role == "student-deliverable")?
        .path_template
        .as_deref()?;
    let marker = template.find("{student_id}")?;
    let prefix = template[..marker].trim_end_matches('/');
    (!prefix.is_empty()).then(|| prefix.to_owned())
}

pub struct LocalFilesystemAgent {
    root: PathBuf,
    limits: DeliveryLimits,
    staged_paths: std::collections::HashSet<PathBuf>,
    borrowed_paths: std::collections::HashSet<PathBuf>,
}

impl LocalFilesystemAgent {
    pub fn new(project_root: &Path, configured_path: &str) -> Result<Self, DeliveryError> {
        let configured = Path::new(configured_path);
        let source = if configured.is_absolute() {
            configured.to_path_buf()
        } else {
            project_root.join(configured)
        };
        let metadata = fs::symlink_metadata(&source)?;
        if metadata.file_type().is_symlink() || !metadata.is_dir() {
            return Err(DeliveryError::Source(format!(
                "local delivery folder must be a real directory: {}",
                source.display()
            )));
        }
        let root = source.canonicalize()?;
        Self::with_root(root, &DeliveryLimits::default())
    }

    pub fn with_limits(
        project_root: &Path,
        configured_path: &str,
        limits: &DeliveryLimits,
    ) -> Result<Self, DeliveryError> {
        let configured = Path::new(configured_path);
        let source = if configured.is_absolute() {
            configured.to_path_buf()
        } else {
            project_root.join(configured)
        };
        let metadata = fs::symlink_metadata(&source)?;
        if metadata.file_type().is_symlink() || !metadata.is_dir() {
            return Err(DeliveryError::Source(format!(
                "local delivery folder must be a real directory: {}",
                source.display()
            )));
        }
        Self::with_root(source.canonicalize()?, limits)
    }

    fn with_root(root: PathBuf, limits: &DeliveryLimits) -> Result<Self, DeliveryError> {
        validate_limits(limits)?;
        Ok(Self {
            root,
            limits: limits.clone(),
            staged_paths: std::collections::HashSet::new(),
            borrowed_paths: std::collections::HashSet::new(),
        })
    }

    fn resolve(&self, name: &str) -> Result<PathBuf, DeliveryError> {
        if name.is_empty()
            || name == "."
            || name == ".."
            || name.contains('/')
            || name.contains('\\')
            || name.contains('\0')
        {
            return Err(DeliveryError::Source(format!(
                "invalid deliverable name {name:?}"
            )));
        }
        let path = self.root.join(name);
        let metadata = fs::symlink_metadata(&path).map_err(|error| {
            DeliveryError::Source(format!("deliverable {name:?} is unavailable: {error}"))
        })?;
        if metadata.file_type().is_symlink() || !(metadata.is_file() || metadata.is_dir()) {
            return Err(DeliveryError::Source(format!(
                "deliverable {name:?} must be a regular file or directory"
            )));
        }
        Ok(path)
    }
}

impl DeliveryAgent for LocalFilesystemAgent {
    fn list(&mut self) -> Result<Vec<DeliveryInfo>, DeliveryError> {
        let mut deliveries = Vec::new();
        for entry in fs::read_dir(&self.root)? {
            let entry = entry?;
            let ty = entry.file_type()?;
            if ty.is_symlink() || !(ty.is_file() || ty.is_dir()) {
                continue;
            }
            let name = entry.file_name().to_string_lossy().into_owned();
            let path = entry.path();
            let (size_bytes, latest_modified) = tree_size_and_modified(&path)?;
            let metadata = entry.metadata()?;
            deliveries.push(DeliveryInfo {
                name,
                size_bytes,
                created_at: metadata.created().ok().and_then(system_time_string),
                modified_at: latest_modified
                    .or_else(|| metadata.modified().ok().and_then(system_time_string)),
                provider_id: None,
                revision: None,
                mime_type: None,
                source_checksum: None,
                source_checksum_algorithm: None,
            });
        }
        deliveries.sort_by(|left, right| left.name.cmp(&right.name));
        Ok(deliveries)
    }

    fn get(&mut self, name: &str, destination: &Path) -> Result<RetrievedDelivery, DeliveryError> {
        let source = self.resolve(name)?;
        let info = self
            .list()?
            .into_iter()
            .find(|item| item.name == name)
            .ok_or_else(|| DeliveryError::Source(format!("deliverable {name:?} disappeared")))?;
        if info.size_bytes > self.limits.max_bytes {
            return Err(DeliveryError::Source(format!(
                "deliverable {:?} exceeds the configured {} byte intake limit",
                info.name, self.limits.max_bytes
            )));
        }
        if tree_file_count(&source, self.limits.max_files)? > self.limits.max_files {
            return Err(DeliveryError::Source(format!(
                "deliverable {name:?} exceeds the configured {} file intake limit",
                self.limits.max_files
            )));
        }
        if let Some(parent) = destination
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty())
        {
            fs::create_dir_all(parent)?;
        }
        let mut budget = CopyBudget::new(&self.limits);
        copy_tree(&source, destination, &mut budget)?;
        let sha256 = digest_tree(destination)?;
        self.staged_paths.insert(destination.to_path_buf());
        Ok(RetrievedDelivery {
            info,
            sha256,
            path: destination.to_path_buf(),
        })
    }

    fn release(&mut self, delivery: &RetrievedDelivery) -> Result<(), DeliveryError> {
        if self.borrowed_paths.remove(&delivery.path) {
            return Ok(());
        }
        release_staged_path(&mut self.staged_paths, delivery)
    }

    fn get_for_execution(
        &mut self,
        name: &str,
        _destination: &Path,
    ) -> Result<RetrievedDelivery, DeliveryError> {
        let source = self.resolve(name)?;
        let info = self
            .list()?
            .into_iter()
            .find(|item| item.name == name)
            .ok_or_else(|| DeliveryError::Source(format!("deliverable {name:?} disappeared")))?;
        if info.size_bytes > self.limits.max_bytes {
            return Err(DeliveryError::Source(format!(
                "deliverable {:?} exceeds the configured {} byte intake limit",
                info.name, self.limits.max_bytes
            )));
        }
        if tree_file_count(&source, self.limits.max_files)? > self.limits.max_files {
            return Err(DeliveryError::Source(format!(
                "deliverable {name:?} exceeds the configured {} file intake limit",
                self.limits.max_files
            )));
        }
        let sha256 = digest_tree(&source)?;
        self.borrowed_paths.insert(source.clone());
        Ok(RetrievedDelivery {
            info,
            sha256,
            path: source,
        })
    }
}

fn release_staged_path(
    staged_paths: &mut std::collections::HashSet<PathBuf>,
    delivery: &RetrievedDelivery,
) -> Result<(), DeliveryError> {
    if !staged_paths.remove(&delivery.path) {
        return Err(DeliveryError::Source(
            "refusing to remove a path not staged by this delivery agent".into(),
        ));
    }
    match fs::symlink_metadata(&delivery.path) {
        Ok(metadata) if metadata.is_dir() && !metadata.file_type().is_symlink() => {
            fs::remove_dir_all(&delivery.path)?;
        }
        Ok(_) => fs::remove_file(&delivery.path)?,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(error.into()),
    }
    Ok(())
}

fn tree_size_and_modified(path: &Path) -> Result<(u64, Option<String>), DeliveryError> {
    let metadata = fs::symlink_metadata(path)?;
    if metadata.file_type().is_symlink() {
        return Err(DeliveryError::Source(format!(
            "symlink in delivery tree: {}",
            path.display()
        )));
    }
    if metadata.is_file() {
        return Ok((
            metadata.len(),
            metadata.modified().ok().and_then(system_time_string),
        ));
    }
    let mut size = 0u64;
    let mut modified: Option<String> = metadata.modified().ok().and_then(system_time_string);
    let mut entries = fs::read_dir(path)?.collect::<Result<Vec<_>, _>>()?;
    entries.sort_by_key(|entry| entry.file_name());
    for entry in entries {
        let (child_size, child_modified) = tree_size_and_modified(&entry.path())?;
        size = size.saturating_add(child_size);
        if child_modified > modified {
            modified = child_modified;
        }
    }
    Ok((size, modified))
}

fn tree_file_count(path: &Path, max_files: usize) -> Result<usize, DeliveryError> {
    let metadata = fs::symlink_metadata(path)?;
    if metadata.file_type().is_symlink() {
        return Err(DeliveryError::Source(format!(
            "symlink in delivery tree: {}",
            path.display()
        )));
    }
    if metadata.is_file() {
        return Ok(1);
    }
    let mut count = 0usize;
    for entry in fs::read_dir(path)? {
        count = count.saturating_add(tree_file_count(&entry?.path(), max_files)?);
        if count > max_files {
            return Ok(count);
        }
    }
    Ok(count)
}

struct CopyBudget {
    bytes: u64,
    files: usize,
    max_bytes: u64,
    max_files: usize,
}

impl CopyBudget {
    fn new(limits: &DeliveryLimits) -> Self {
        Self {
            bytes: 0,
            files: 0,
            max_bytes: limits.max_bytes,
            max_files: limits.max_files,
        }
    }
}

fn copy_tree(
    source: &Path,
    destination: &Path,
    budget: &mut CopyBudget,
) -> Result<(), DeliveryError> {
    let metadata = fs::symlink_metadata(source)?;
    if metadata.file_type().is_symlink() {
        return Err(DeliveryError::Source(format!(
            "refusing symlink {}",
            source.display()
        )));
    }
    if metadata.is_file() {
        budget.files = budget.files.saturating_add(1);
        if budget.files > budget.max_files
            || metadata.len() > budget.max_bytes.saturating_sub(budget.bytes)
        {
            return Err(DeliveryError::Source(
                "deliverable exceeded intake file or byte limits while copying".into(),
            ));
        }
        if let Some(parent) = destination.parent()
            && !parent.as_os_str().is_empty()
        {
            fs::create_dir_all(parent)?;
        }
        let input = open_source_file(source)?;
        let mut output = OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(destination)?;
        let remaining = budget.max_bytes.saturating_sub(budget.bytes);
        let copied = match std::io::copy(&mut input.take(remaining.saturating_add(1)), &mut output)
        {
            Ok(bytes) if bytes <= remaining => bytes,
            Ok(_) => {
                let _ = fs::remove_file(destination);
                return Err(DeliveryError::Source(format!(
                    "deliverable exceeded the {} byte intake limit while copying",
                    budget.max_bytes
                )));
            }
            Err(error) => {
                let _ = fs::remove_file(destination);
                return Err(error.into());
            }
        };
        if let Err(error) = output.sync_all() {
            let _ = fs::remove_file(destination);
            return Err(error.into());
        }
        budget.bytes = budget.bytes.saturating_add(copied);
        return Ok(());
    }
    if !metadata.is_dir() {
        return Err(DeliveryError::Source(format!(
            "unsupported file type {}",
            source.display()
        )));
    }
    fs::create_dir(destination)?;
    let mut entries = fs::read_dir(source)?.collect::<Result<Vec<_>, _>>()?;
    entries.sort_by_key(|entry| entry.file_name());
    for entry in entries {
        copy_tree(&entry.path(), &destination.join(entry.file_name()), budget)?;
    }
    Ok(())
}

fn digest_tree(path: &Path) -> Result<String, DeliveryError> {
    fn visit(path: &Path, relative: &Path, hash: &mut Sha256) -> Result<(), DeliveryError> {
        let metadata = fs::symlink_metadata(path)?;
        if metadata.file_type().is_symlink() {
            return Err(DeliveryError::Source(format!(
                "symlink in delivery tree: {}",
                path.display()
            )));
        }
        if metadata.is_file() {
            if !relative.as_os_str().is_empty() {
                hash.update(relative.to_string_lossy().as_bytes());
                hash.update([0]);
            }
            let mut file = open_source_file(path)?;
            let mut buffer = [0u8; 32 * 1024];
            loop {
                let read = file.read(&mut buffer)?;
                if read == 0 {
                    break;
                }
                hash.update(&buffer[..read]);
            }
        } else if metadata.is_dir() {
            let mut entries = fs::read_dir(path)?.collect::<Result<Vec<_>, _>>()?;
            entries.sort_by_key(|entry| entry.file_name());
            for entry in entries {
                visit(&entry.path(), &relative.join(entry.file_name()), hash)?;
            }
        } else {
            return Err(DeliveryError::Source(format!(
                "unsupported file type {}",
                path.display()
            )));
        }
        Ok(())
    }
    let mut hash = Sha256::new();
    visit(path, Path::new(""), &mut hash)?;
    Ok(format!("{:x}", hash.finalize()))
}

fn system_time_string(value: SystemTime) -> Option<String> {
    let duration = value.duration_since(UNIX_EPOCH).ok()?;
    DateTime::<Utc>::from_timestamp(duration.as_secs() as i64, duration.subsec_nanos())
        .map(|time| time.to_rfc3339())
}

pub struct GoogleDriveAgent {
    folder_id: String,
    credentials: DriveCredentials,
    client: Client,
    limits: DeliveryLimits,
    staged_paths: std::collections::HashSet<PathBuf>,
}

impl GoogleDriveAgent {
    pub fn new(folder_url: &str) -> Result<Self, DeliveryError> {
        Self::with_limits(folder_url, &DeliveryLimits::default())
    }

    pub fn with_limits(folder_url: &str, limits: &DeliveryLimits) -> Result<Self, DeliveryError> {
        validate_limits(limits)?;
        let folder_id = parse_folder_id(folder_url)?;
        let credentials = DriveCredentials::load()?;
        let client = Client::builder()
            .timeout(Duration::from_secs(60 * 60))
            .user_agent(concat!("thot/", env!("CARGO_PKG_VERSION")))
            .build()
            .map_err(|error| DeliveryError::Remote(error.to_string()))?;
        Ok(Self {
            folder_id,
            credentials,
            client,
            limits: limits.clone(),
            staged_paths: std::collections::HashSet::new(),
        })
    }

    fn access_token(&mut self) -> Result<String, DeliveryError> {
        if self.credentials.expires_at > now_epoch().saturating_add(60) {
            return Ok(self.credentials.access_token.clone());
        }
        let client_id = self.credentials.client_id.clone();
        let client_secret = self.credentials.client_secret.clone();
        if client_id.is_empty() || client_secret.is_empty() {
            return Err(DeliveryError::Authorization("Google OAuth client credentials are unavailable; configure the developer OAuth client at build time or set THOT_GDRIVE_CLIENT_ID and THOT_GDRIVE_CLIENT_SECRET, then run `thot deliveries list` to authorize".into()));
        }
        if let Some(refresh_token) = self.credentials.refresh_token.clone() {
            let response = self
                .client
                .post("https://oauth2.googleapis.com/token")
                .form(&[
                    ("client_id", client_id.as_str()),
                    ("client_secret", client_secret.as_str()),
                    ("refresh_token", refresh_token.as_str()),
                    ("grant_type", "refresh_token"),
                ])
                .send()
                .map_err(|error| DeliveryError::Authorization(error.to_string()))?;
            if response.status().is_success() {
                let token: TokenResponse = response
                    .json()
                    .map_err(|error| DeliveryError::Authorization(error.to_string()))?;
                self.credentials.access_token = token.access_token;
                self.credentials.expires_at = now_epoch().saturating_add(token.expires_in);
                if !is_sudo_invocation() {
                    self.credentials.save()?;
                }
                return Ok(self.credentials.access_token.clone());
            }
        }
        self.credentials = oauth_authorize(&self.client, &client_id, &client_secret)?;
        self.credentials.save()?;
        Ok(self.credentials.access_token.clone())
    }

    fn list_remote(&mut self) -> Result<Vec<DriveFile>, DeliveryError> {
        let token = self.access_token()?;
        let mut page_token: Option<String> = None;
        let mut files = Vec::new();
        loop {
            let mut url = Url::parse("https://www.googleapis.com/drive/v3/files")
                .map_err(|error| DeliveryError::Remote(error.to_string()))?;
            {
                let mut query = url.query_pairs_mut();
                query.append_pair(
                    "q",
                    &format!("'{}' in parents and trashed = false", self.folder_id),
                );
                query.append_pair("pageSize", "1000");
                query.append_pair("orderBy", "name");
                query.append_pair(
                    "fields",
                    "nextPageToken,files(id,name,size,createdTime,modifiedTime,mimeType,version,headRevisionId,md5Checksum)",
                );
                query.append_pair("includeItemsFromAllDrives", "true");
                query.append_pair("supportsAllDrives", "true");
                if let Some(page) = &page_token {
                    query.append_pair("pageToken", page);
                }
            }
            let response = self
                .client
                .get(url)
                .bearer_auth(&token)
                .send()
                .map_err(|error| DeliveryError::Remote(error.to_string()))?;
            let status = response.status();
            if !status.is_success() {
                return Err(remote_error(response)?);
            }
            let page: DriveListResponse = response
                .json()
                .map_err(|error| DeliveryError::Remote(error.to_string()))?;
            files.extend(page.files.into_iter().filter(|file| {
                file.mime_type != "application/vnd.google-apps.folder"
                    && !file.mime_type.starts_with("application/vnd.google-apps.")
            }));
            page_token = page.next_page_token;
            if page_token.is_none() {
                break;
            }
        }
        Ok(files)
    }
}

impl DeliveryAgent for GoogleDriveAgent {
    fn list(&mut self) -> Result<Vec<DeliveryInfo>, DeliveryError> {
        let files = self.list_remote()?;
        let mut result = files.iter().map(DriveFile::info).collect::<Vec<_>>();
        result.sort_by(|left, right| left.name.cmp(&right.name));
        Ok(result)
    }

    fn get(&mut self, name: &str, destination: &Path) -> Result<RetrievedDelivery, DeliveryError> {
        let files = self.list_remote()?;
        let matches = files
            .into_iter()
            .filter(|file| file.name == name)
            .collect::<Vec<_>>();
        if matches.len() != 1 {
            return Err(DeliveryError::Source(if matches.is_empty() {
                format!("Google Drive has no deliverable named {name:?}")
            } else {
                format!(
                    "Google Drive has multiple deliverables named {name:?}; names must be unique"
                )
            }));
        }
        let delivery = download_drive_file(self, matches.into_iter().next().unwrap(), destination)?;
        self.staged_paths.insert(delivery.path.clone());
        Ok(delivery)
    }

    fn get_by_id(
        &mut self,
        provider_id: &str,
        destination: &Path,
    ) -> Result<RetrievedDelivery, DeliveryError> {
        let file = self
            .list_remote()?
            .into_iter()
            .find(|file| file.id == provider_id)
            .ok_or_else(|| {
                DeliveryError::Source(format!(
                    "Google Drive has no deliverable with file ID {provider_id:?}"
                ))
            })?;
        let delivery = download_drive_file(self, file, destination)?;
        self.staged_paths.insert(delivery.path.clone());
        Ok(delivery)
    }

    fn release(&mut self, delivery: &RetrievedDelivery) -> Result<(), DeliveryError> {
        release_staged_path(&mut self.staged_paths, delivery)
    }
}

fn download_drive_file(
    agent: &mut GoogleDriveAgent,
    file: DriveFile,
    destination: &Path,
) -> Result<RetrievedDelivery, DeliveryError> {
    let info = file.info();
    if info.size_bytes > agent.limits.max_bytes {
        return Err(DeliveryError::Source(format!(
            "deliverable {:?} exceeds the configured {} byte intake limit",
            info.name, agent.limits.max_bytes
        )));
    }
    let token = agent.access_token()?;
    let url = match file.head_revision_id.as_deref() {
        Some(revision_id) => format!(
            "https://www.googleapis.com/drive/v3/files/{}/revisions/{}?alt=media&supportsAllDrives=true",
            file.id, revision_id
        ),
        None => format!(
            "https://www.googleapis.com/drive/v3/files/{}?alt=media&supportsAllDrives=true",
            file.id
        ),
    };
    let response = agent
        .client
        .get(url)
        .bearer_auth(token)
        .send()
        .map_err(|error| DeliveryError::Remote(error.to_string()))?;
    let response = checked_response(response)?;
    let mut output = create_new_destination(destination)?;
    let mut limited = response.take(agent.limits.max_bytes.saturating_add(1));
    let mut total = 0u64;
    let mut hash = Sha256::new();
    let mut buffer = [0u8; 64 * 1024];
    loop {
        let read = match limited.read(&mut buffer) {
            Ok(read) => read,
            Err(error) => {
                let _ = fs::remove_file(destination);
                return Err(error.into());
            }
        };
        if read == 0 {
            break;
        }
        total = total.saturating_add(read as u64);
        if total > agent.limits.max_bytes {
            let _ = fs::remove_file(destination);
            return Err(DeliveryError::Source(format!(
                "download exceeded the configured {} byte intake limit",
                agent.limits.max_bytes
            )));
        }
        if let Err(error) = output.write_all(&buffer[..read]) {
            let _ = fs::remove_file(destination);
            return Err(error.into());
        }
        hash.update(&buffer[..read]);
    }
    if let Err(error) = output.sync_all() {
        let _ = fs::remove_file(destination);
        return Err(error.into());
    }
    let sha256 = format!("{:x}", hash.finalize());
    Ok(RetrievedDelivery {
        info,
        sha256,
        path: destination.to_path_buf(),
    })
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct DriveCredentials {
    client_id: String,
    #[serde(default)]
    client_secret: String,
    access_token: String,
    refresh_token: Option<String>,
    expires_at: u64,
}

impl DriveCredentials {
    fn path() -> Result<PathBuf, DeliveryError> {
        if let Some(path) = std::env::var_os("THOT_GDRIVE_TOKEN_FILE") {
            return Ok(PathBuf::from(path));
        }
        let base = if let Ok(user) = std::env::var("SUDO_USER") {
            let output = Command::new("getent")
                .args(["passwd", &user])
                .output()
                .map_err(|error| {
                    DeliveryError::Authorization(format!(
                        "cannot find the invoking professor's home directory: {error}"
                    ))
                })?;
            let record = String::from_utf8(output.stdout)
                .map_err(|error| DeliveryError::Authorization(error.to_string()))?;
            let home = record
                .split(':')
                .nth(5)
                .filter(|home| !home.is_empty())
                .ok_or_else(|| {
                    DeliveryError::Authorization("cannot resolve SUDO_USER home directory".into())
                })?;
            PathBuf::from(home).join(".config")
        } else if let Some(path) = std::env::var_os("XDG_CONFIG_HOME") {
            PathBuf::from(path)
        } else {
            std::env::var_os("HOME")
                .map(|home| PathBuf::from(home).join(".config"))
                .ok_or_else(|| {
                    DeliveryError::Authorization(
                        "HOME or XDG_CONFIG_HOME must be set to store the private OAuth token"
                            .into(),
                    )
                })?
        };
        Ok(base.join("thot").join("google-drive-token.json"))
    }

    fn load() -> Result<Self, DeliveryError> {
        let path = Self::path()?;
        let env_client_id = std::env::var("THOT_GDRIVE_CLIENT_ID")
            .ok()
            .or_else(|| option_env!("THOT_GDRIVE_BUNDLED_CLIENT_ID").map(str::to_owned));
        let env_client_secret = std::env::var("THOT_GDRIVE_CLIENT_SECRET")
            .ok()
            .or_else(|| option_env!("THOT_GDRIVE_BUNDLED_CLIENT_SECRET").map(str::to_owned));
        let cached = match open_source_file(&path) {
            Ok(mut file) => {
                #[cfg(unix)]
                {
                    use std::os::unix::fs::MetadataExt;
                    let metadata = file.metadata()?;
                    let expected_owner = std::env::var("SUDO_UID")
                        .ok()
                        .and_then(|uid| uid.parse::<u32>().ok())
                        .unwrap_or_else(|| unsafe { libc::geteuid() });
                    if metadata.uid() != expected_owner || metadata.mode() & 0o077 != 0 {
                        return Err(DeliveryError::Authorization(format!(
                            "OAuth token cache {} must be owned by the professor and have mode 0600",
                            path.display()
                        )));
                    }
                }
                let mut bytes = Vec::new();
                file.read_to_end(&mut bytes)?;
                Some(bytes)
            }
            Err(DeliveryError::Io(error)) if error.kind() == std::io::ErrorKind::NotFound => None,
            Err(error) => return Err(error),
        };
        match cached {
            Some(bytes) => {
                let mut credentials: Self = serde_json::from_slice(&bytes).map_err(|error| {
                    DeliveryError::Authorization(format!(
                        "invalid token cache {}: {error}",
                        path.display()
                    ))
                })?;
                if env_client_id
                    .as_ref()
                    .is_some_and(|client_id| credentials.client_id != *client_id)
                {
                    return Err(DeliveryError::Authorization("cached OAuth token belongs to a different Google client ID; remove the token cache and authorize again".into()));
                }
                if let Some(client_id) = env_client_id {
                    credentials.client_id = client_id;
                }
                if let Some(client_secret) = env_client_secret {
                    credentials.client_secret = client_secret;
                }
                Ok(credentials)
            }
            None => Ok(Self {
                client_id: env_client_id.unwrap_or_default(),
                client_secret: env_client_secret.unwrap_or_default(),
                access_token: String::new(),
                refresh_token: None,
                expires_at: 0,
            }),
        }
    }

    fn save(&self) -> Result<(), DeliveryError> {
        let path = Self::path()?;
        let parent = path.parent().ok_or_else(|| {
            DeliveryError::Authorization("OAuth token path has no parent directory".into())
        })?;
        fs::create_dir_all(parent)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(parent, fs::Permissions::from_mode(0o700))?;
        }
        let temporary = path.with_extension(format!("tmp-{}", uuid::Uuid::new_v4()));
        let mut file = OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(&temporary)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            file.set_permissions(fs::Permissions::from_mode(0o600))?;
        }
        file.write_all(
            &serde_json::to_vec_pretty(self)
                .map_err(|error| DeliveryError::Authorization(error.to_string()))?,
        )?;
        file.sync_all()?;
        fs::rename(temporary, path)?;
        Ok(())
    }
}

#[derive(Debug, Deserialize)]
struct TokenResponse {
    access_token: String,
    expires_in: u64,
    #[serde(default)]
    refresh_token: Option<String>,
}

#[derive(Debug, Deserialize)]
struct DriveListResponse {
    #[serde(rename = "nextPageToken")]
    next_page_token: Option<String>,
    #[serde(default)]
    files: Vec<DriveFile>,
}

#[derive(Debug, Deserialize)]
struct DriveFile {
    id: String,
    name: String,
    size: Option<String>,
    #[serde(rename = "createdTime")]
    created_time: Option<String>,
    #[serde(rename = "modifiedTime")]
    modified_time: Option<String>,
    #[serde(rename = "mimeType")]
    mime_type: String,
    #[serde(default)]
    version: Option<String>,
    #[serde(rename = "headRevisionId", default)]
    head_revision_id: Option<String>,
    #[serde(rename = "md5Checksum", default)]
    md5_checksum: Option<String>,
}

impl DriveFile {
    fn info(&self) -> DeliveryInfo {
        DeliveryInfo {
            name: self.name.clone(),
            size_bytes: self
                .size
                .as_deref()
                .and_then(|size| size.parse().ok())
                .unwrap_or(0),
            created_at: self.created_time.clone(),
            modified_at: self.modified_time.clone(),
            provider_id: Some(self.id.clone()),
            revision: self
                .head_revision_id
                .clone()
                .or_else(|| self.version.clone()),
            mime_type: Some(self.mime_type.clone()),
            source_checksum: self.md5_checksum.clone(),
            source_checksum_algorithm: self.md5_checksum.as_ref().map(|_| "md5".into()),
        }
    }
}

fn parse_folder_id(value: &str) -> Result<String, DeliveryError> {
    let url = Url::parse(value).map_err(|error| DeliveryError::Configuration(error.to_string()))?;
    if url.scheme() != "https" || url.host_str() != Some("drive.google.com") {
        return Err(DeliveryError::Configuration(
            "Google Drive folder URL must use https://drive.google.com".into(),
        ));
    }
    let segments = url
        .path_segments()
        .ok_or_else(|| DeliveryError::Configuration("invalid Google Drive folder URL".into()))?
        .collect::<Vec<_>>();
    let id = segments
        .windows(2)
        .find(|pair| pair[0] == "folders")
        .map(|pair| pair[1])
        .unwrap_or("");
    if id.is_empty()
        || !id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_' || byte == b'-')
    {
        return Err(DeliveryError::Configuration(
            "Google Drive URL must contain a valid /folders/FOLDER_ID path".into(),
        ));
    }
    Ok(id.to_owned())
}

fn oauth_authorize(
    client: &Client,
    client_id: &str,
    client_secret: &str,
) -> Result<DriveCredentials, DeliveryError> {
    if is_sudo_invocation() {
        return Err(DeliveryError::Authorization(
            "authorize Google Drive as the professor without sudo first (run `thot deliveries list`); privileged sandbox runs can then reuse the private token cache".into(),
        ));
    }
    let listener = std::net::TcpListener::bind(("127.0.0.1", 0))?;
    listener.set_nonblocking(true)?;
    let port = listener.local_addr()?.port();
    let redirect_uri = format!("http://127.0.0.1:{port}/callback");
    let state = uuid::Uuid::new_v4().to_string();
    let verifier = format!(
        "{}{}",
        uuid::Uuid::new_v4().simple(),
        uuid::Uuid::new_v4().simple()
    );
    let challenge = URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()));
    let mut authorization = Url::parse("https://accounts.google.com/o/oauth2/v2/auth")
        .map_err(|error| DeliveryError::Authorization(error.to_string()))?;
    authorization
        .query_pairs_mut()
        .append_pair("client_id", client_id)
        .append_pair("redirect_uri", &redirect_uri)
        .append_pair("response_type", "code")
        .append_pair("scope", DRIVE_READONLY_SCOPE)
        .append_pair("access_type", "offline")
        .append_pair("prompt", "consent")
        .append_pair("state", &state)
        .append_pair("code_challenge", &challenge)
        .append_pair("code_challenge_method", "S256");
    eprintln!(
        "Authorize Thot with read-only Google Drive access: {}",
        authorization
    );
    let _ = Command::new("xdg-open")
        .arg(authorization.as_str())
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn();
    eprintln!("Waiting for the Google authorization redirect on localhost (up to 5 minutes)...");
    let deadline = std::time::Instant::now() + Duration::from_secs(300);
    let (mut stream, _) = loop {
        match listener.accept() {
            Ok(connection) => break connection,
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                if std::time::Instant::now() >= deadline {
                    return Err(DeliveryError::Authorization(
                        "timed out waiting for browser authorization".into(),
                    ));
                }
                std::thread::sleep(Duration::from_millis(200));
            }
            Err(error) => return Err(error.into()),
        }
    };
    stream.set_read_timeout(Some(Duration::from_secs(5)))?;
    let mut request = [0u8; 8192];
    let count = stream.read(&mut request)?;
    let request_line = String::from_utf8_lossy(&request[..count]);
    let target = request_line
        .split_whitespace()
        .nth(1)
        .ok_or_else(|| DeliveryError::Authorization("invalid OAuth callback request".into()))?;
    let callback = Url::parse(&format!("http://127.0.0.1:{port}{target}"))
        .map_err(|error| DeliveryError::Authorization(error.to_string()))?;
    let params = callback
        .query_pairs()
        .collect::<std::collections::HashMap<_, _>>();
    let response_body =
        "Google authorization received. You may close this browser tab and return to Thot.";
    stream.write_all(format!("HTTP/1.1 200 OK\r\nContent-Type: text/plain; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}", response_body.len(), response_body).as_bytes())?;
    if params.get("state").map(|value| value.as_ref()) != Some(state.as_str()) {
        return Err(DeliveryError::Authorization(
            "OAuth state did not match; authorization rejected".into(),
        ));
    }
    let code = params.get("code").ok_or_else(|| {
        DeliveryError::Authorization("Google did not return an authorization code".into())
    })?;
    let response = client
        .post("https://oauth2.googleapis.com/token")
        .form(&[
            ("client_id", client_id),
            ("client_secret", client_secret),
            ("code", code.as_ref()),
            ("grant_type", "authorization_code"),
            ("redirect_uri", redirect_uri.as_str()),
            ("code_verifier", verifier.as_str()),
        ])
        .send()
        .map_err(|error| DeliveryError::Authorization(error.to_string()))?;
    let token = checked_token_response(response)?;
    Ok(DriveCredentials {
        client_id: client_id.to_owned(),
        client_secret: client_secret.to_owned(),
        access_token: token.access_token,
        refresh_token: token.refresh_token,
        expires_at: now_epoch().saturating_add(token.expires_in),
    })
}

fn checked_token_response(response: Response) -> Result<TokenResponse, DeliveryError> {
    let status = response.status();
    if status.is_success() {
        return response
            .json()
            .map_err(|error| DeliveryError::Authorization(error.to_string()));
    }
    let message = response.text().unwrap_or_default();
    Err(DeliveryError::Authorization(format!(
        "Google token endpoint returned {status}: {message}"
    )))
}

fn checked_response(response: Response) -> Result<Response, DeliveryError> {
    let status = response.status();
    if status.is_success() {
        return Ok(response);
    }
    Err(remote_error(response)?)
}

fn remote_error(response: Response) -> Result<DeliveryError, DeliveryError> {
    let status = response.status();
    let message = response.text().unwrap_or_default();
    Ok(DeliveryError::Remote(format!(
        "Google Drive returned {status}: {message}"
    )))
}

fn create_new_destination(destination: &Path) -> Result<File, DeliveryError> {
    if let Some(parent) = destination
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
    {
        fs::create_dir_all(parent)?;
    }
    OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(destination)
        .map_err(Into::into)
}

fn open_source_file(path: &Path) -> Result<File, DeliveryError> {
    let mut options = OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC);
    }
    options.open(path).map_err(Into::into)
}

fn now_epoch() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

fn is_sudo_invocation() -> bool {
    (unsafe { libc::geteuid() }) == 0 && std::env::var_os("SUDO_USER").is_some()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::symlink;

    #[test]
    fn local_agent_lists_and_gets_files_and_directory_deliverables() {
        let root = std::env::temp_dir().join(format!("thot-delivery-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(root.join("submission")).unwrap();
        fs::write(root.join("one.zip"), b"archive").unwrap();
        fs::write(root.join("submission/main.sh"), b"echo ok\n").unwrap();
        let mut agent = LocalFilesystemAgent::new(&root, ".").unwrap();
        let files = agent.list().unwrap();
        assert_eq!(
            files
                .iter()
                .find(|item| item.name == "one.zip")
                .unwrap()
                .size_bytes,
            7
        );
        assert_eq!(
            files
                .iter()
                .find(|item| item.name == "submission")
                .unwrap()
                .size_bytes,
            8
        );
        let retrieved = agent
            .get("submission", &root.join("out/submission"))
            .unwrap();
        assert_eq!(retrieved.info.name, "submission");
        assert_eq!(
            fs::read(retrieved.path.join("main.sh")).unwrap(),
            b"echo ok\n"
        );
        let archive = agent.get("one.zip", &root.join("out/one.zip")).unwrap();
        assert_eq!(archive.sha256, format!("{:x}", Sha256::digest(b"archive")));
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn local_agent_rejects_traversal_and_symlinks() {
        let root = std::env::temp_dir().join(format!("thot-delivery-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&root).unwrap();
        fs::write(root.join("safe"), b"x").unwrap();
        symlink("safe", root.join("link")).unwrap();
        let mut agent = LocalFilesystemAgent::new(&root, ".").unwrap();
        assert!(agent.get("../safe", &root.join("out")).is_err());
        assert!(agent.get("link", &root.join("out")).is_err());
        symlink("missing", root.join("output")).unwrap();
        assert!(agent.get("safe", &root.join("output")).is_err());
        assert!(parse_folder_id("https://drive.google.com/drive/folders/abc_123").is_ok());
        assert!(parse_folder_id("https://example.com/drive/folders/abc").is_err());
        let _ = fs::remove_dir_all(root);
    }
}
