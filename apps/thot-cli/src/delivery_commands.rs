//! Purpose: Parses run and expedient CLI commands and calls the corresponding API routes.
use crate::{Settings, api_client::ApiClient};
use reqwest::Method;
use serde_json::{Value, json};
use std::path::Path;

pub async fn handle(
    args: &[String],
    settings: &Settings,
) -> Result<(), Box<dyn std::error::Error>> {
    let client = ApiClient::new(settings)?;
    let (method, path, body) = match args {
        [] => (Method::GET, "/api/v1/runs".to_owned(), None),
        [command] if command == "list" => {
            (Method::GET, "/api/v1/runs".to_owned(), None)
        }
        [action, project_file] if action == "create" => (
            Method::POST,
            "/api/v1/runs".to_owned(),
            Some(json!({"project_path":absolute_path(project_file)?})),
        ),
        [action, project_file, flag_deliveries, directory]
            if action == "create" && flag_deliveries == "--deliveries" =>
        {
            let body = json!({
                "project_path": absolute_path(project_file)?,
                "delivery_directory": absolute_path(directory)?
            });
            (Method::POST, "/api/v1/runs".to_owned(), Some(body))
        }
        [action, id] if action == "status" => (Method::GET, format!("/api/v1/runs/{id}"), None),
        [action, id] if action == "delete" => {
            (Method::DELETE, format!("/api/v1/runs/{id}"), None)
        }
        [action, id] if action == "expedient" => (Method::GET, format!("/api/v1/expedients/{id}"), None),
        [action, id] if action == "cancel" => (Method::POST, format!("/api/v1/runs/{id}/cancel"), Some(json!({}))),
        [action, id] if action == "cleanup" => (Method::POST, format!("/api/v1/expedients/{id}/cleanup"), Some(json!({}))),
        _ => return Err("usage: thot run [list] | create <project-file> [--deliveries DIR] | status RUN_ID | delete RUN_ID | expedient EXPEDIENT_ID | cancel RUN_ID | cleanup EXPEDIENT_ID".into()),
    };
    let value: Value = client.request(method, &path, body).await?;
    println!("{}", serde_json::to_string_pretty(&value)?);
    Ok(())
}

fn absolute_path(path: &str) -> Result<String, Box<dyn std::error::Error>> {
    let path = Path::new(path);
    let absolute = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir()?.join(path)
    };
    Ok(absolute.to_string_lossy().into_owned())
}
