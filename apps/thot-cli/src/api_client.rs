//! Purpose: Sends CLI requests to the Thot HTTP API and decodes its responses.
use crate::Settings;
use reqwest::{Client, Method, StatusCode};
use serde_json::{Value, json};
use std::time::Duration;

pub struct ApiClient {
    client: Client,
    base_url: String,
}

impl ApiClient {
    pub fn new(settings: &Settings) -> Result<Self, reqwest::Error> {
        Ok(Self {
            client: Client::builder().timeout(Duration::from_secs(30)).build()?,
            base_url: format!("http://{}:{}", settings.host, settings.port),
        })
    }

    pub async fn request(
        &self,
        method: Method,
        path: &str,
        body: Option<Value>,
    ) -> Result<Value, Box<dyn std::error::Error>> {
        let url = format!("{}{}", self.base_url, path);
        let mut request = self.client.request(method, url);
        if let Some(body) = body {
            request = request.json(&body);
        }
        let response = request.send().await?;
        let status = response.status();
        let bytes = response.bytes().await?;
        if !status.is_success() {
            let message = serde_json::from_slice::<Value>(&bytes)
                .ok()
                .and_then(|value| {
                    value
                        .get("error")
                        .and_then(Value::as_str)
                        .map(ToOwned::to_owned)
                })
                .unwrap_or_else(|| String::from_utf8_lossy(&bytes).into_owned());
            return Err(format!("thotd returned HTTP {}: {}", status, message).into());
        }
        if status == StatusCode::NO_CONTENT || bytes.is_empty() {
            return Ok(json!({"status":"ok"}));
        }
        Ok(serde_json::from_slice(&bytes)?)
    }
}
