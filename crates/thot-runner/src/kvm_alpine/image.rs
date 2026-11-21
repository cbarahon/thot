use std::fs::File;
use std::io::{self, Write};
use std::path::Path;
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
    let version = "v3.14";
    let iso_url = format!("{}/releases/x86_64/alpine-standard-{}.iso", mirror, version);
    let checksum_url = format!("{}/releases/x86_64/alpine-standard-{}.iso.sha256", mirror, version);

    let expected_digest = get_expected_digest(&checksum_url).await?;

    let cache_path = cache_root.join(format!("{}.iso", image));
    let temp_path = cache_path.with_extension("tmp");

    if !cache_path.exists() || !verify_iso(&cache_path, &expected_digest).await? {
        download_and_verify(&iso_url, &temp_path, &expected_digest, limits).await?;
        std::fs::rename(&temp_path, &cache_path).map_err(|e| PlatformError::from(e))?;
    }

    Ok(VerifiedImage {
        path: cache_path,
        url: iso_url,
        digest: expected_digest,
    })
}

async fn get_expected_digest(url: &str) -> Result<String, PlatformError> {
    let response = reqwest::get(url).await?;
    let text = response.text().await?;
    let parts: Vec<&str> = text.split_whitespace().collect();
    if parts.len() != 2 {
        return Err(PlatformError::InvalidChecksum);
    }
    Ok(parts[0].to_string())
}

async fn download_and_verify(
    url: &str,
    temp_path: &Path,
    expected_digest: &str,
    limits: &Limits,
) -> Result<(), PlatformError> {
    let mut file = AsyncFile::create(temp_path).await?;
    let mut reader = reqwest::get(url).await?.into_stream();
    let mut total_bytes = 0;

    while let Some(chunk) = reader.next().await {
        let chunk = chunk?;
        total_bytes += chunk.len() as u64;
        if total_bytes > limits.max_size {
            return Err(PlatformError::SizeLimitExceeded);
        }
        file.write_all(&chunk).await?;
    }

    file.sync_all().await?;
    verify_iso(temp_path, expected_digest).await?;
    Ok(())
}

async fn verify_iso(path: &Path, expected_digest: &str) -> Result<bool, PlatformError> {
    let mut file = File::open(path)?;
    let mut digest = Sha256::new();
    let mut buffer = [0; 4096];

    loop {
        let bytes_read = file.read(&mut buffer).await?;
        if bytes_read == 0 {
            break;
        }
        digest.update(&buffer[..bytes_read]);
    }

    let actual_digest = format!("{:x}", digest.finalize());
    Ok(actual_digest == expected_digest)
}

pub struct Limits {
    pub max_size: u64,
}