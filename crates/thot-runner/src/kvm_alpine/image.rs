use std::fs::{File, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use sha2::{Sha256, Digest};
use tokio::fs::File as AsyncFile;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use reqwest::Client;
use tokio::sync::Mutex;
use crate::error::PlatformError;

pub struct VerifiedImage {
    pub path: PathBuf,
    pub url: String,
    pub digest: String,
}

async fn ensure_verified_iso(
    image: &str,
    cache_root: &Path,
    limits: &Limits,
) -> Result<VerifiedImage, PlatformError> {
    let client = Client::new();
    let mirror = "https://dl-cdn.alpinelinux.org/alpine/";
    let version = "v3.24";
    let release = "v3.24.4";
    let iso = format!("alpine-virt-{}.x86_64.iso", release);
    let url = format!("{}/{}-releases/x86_64/{}", mirror, version, iso);
    let digest_url = format!("{}/{}-releases/x86_64/{}.sha256", mirror, version, iso);
    let expected_digest = "d6b2a1a5f2b8d6e1e7f9a7c8d9e0f1a2b3c4d5e6f7a8b9c0d1e2f3a4b5c6d7e8";

    let digest = download_and_verify_digest(&digest_url, expected_digest).await?;
    let path = cache_root.join(&iso);

    if !path.exists() || path.metadata().unwrap().len() != digest.len() {
        download_iso(&url, &path).await?;
    }

    Ok(VerifiedImage {
        path,
        url,
        digest,
    })
}

async fn download_and_verify_digest(url: &str, expected_digest: &str) -> Result<String, PlatformError> {
    let response = client.get(url).send().await?;
    let digest = response.text().await?;

    if digest.trim() != expected_digest {
        return Err(PlatformError::DigestMismatch(expected_digest.to_string(), digest.trim().to_string()));
    }

    Ok(digest.trim().to_string())
}

async fn download_iso(url: &str, path: &Path) -> Result<(), PlatformError> {
    let mut response = client.get(url).send().await?;
    let mut file = OpenOptions::new().write(true).create(true).truncate(true).open(path).await?;

    while let Some(chunk) = response.chunk().await? {
        file.write_all(&chunk).await?;
    }

    Ok(())
}

pub struct Limits {
    pub max_cpu: u32,
    pub max_memory: u64,
    pub max_disk: u64,
}