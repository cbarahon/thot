use std::fs::{File, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use sha2::{Sha256, Digest};
use tokio::fs::File as AsyncFile;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use reqwest::Client;
use tokio::sync::Mutex;
use crate::error::ProviderError;

pub struct VerifiedImage {
    pub path: PathBuf,
    pub url: String,
    pub sha256: String,
}

pub async fn ensure_verified_iso(settings: &KvmAlpineSettings, image: &Image) -> Result<VerifiedImage, ProviderError> {
    let client = Client::new();
    let url = image.url;
    let sha256_url = format!("{}{}.sha256", url.rsplit('/').next().unwrap_or_default(), url.split('/').last().unwrap_or_default());
    let expected_sha256 = image.sha256;

    let actual_sha256 = download_and_verify_sha256(&sha256_url, expected_sha256).await?;
    let path = settings.iso_cache.join(image.filename);

    if !path.exists() || path.metadata().unwrap().len() != actual_sha256.len() {
        download_iso(&url, &path).await?;
    }

    Ok(VerifiedImage {
        path,
        url,
        sha256: actual_sha256,
    })
}

async fn download_and_verify_sha256(url: &str, expected_sha256: &str) -> Result<String, ProviderError> {
    let response = client.get(url).send().await?;
    let sha256 = response.text().await?;

    if sha256.trim() != expected_sha256 {
        return Err(ProviderError::DigestMismatch(expected_sha256.to_string(), sha256.trim().to_string()));
    }

    Ok(sha256.trim().to_string())
}

async fn download_iso(url: &str, path: &Path) -> Result<(), ProviderError> {
    let mut response = client.get(url).send().await?;
    let mut file = OpenOptions::new().write(true).create(true).truncate(true).open(path).await?;

    while let Some(chunk) = response.chunk().await? {
        file.write_all(&chunk).await?;
    }

    Ok(())
}

pub struct KvmAlpineSettings {
    pub alpine_signing_keyring: String,
    pub alpine_signing_fingerprint: String,
    pub allowed_mirror_hosts: Vec<String>,
    pub max_iso_bytes: u64,
}

pub struct Image {
    pub url: String,
    pub filename: String,
    pub sha256: String,
}

pub struct Limits {
    pub max_cpu: u32,
    pub max_memory: u64,
    pub max_disk: u64,
}

// Tests would be added here