use crate::{Settings, api_client::ApiClient};
use reqwest::Method;
use serde_json::{Value, json};

pub async fn handle(
    args: &[String],
    settings: &Settings,
) -> Result<(), Box<dyn std::error::Error>> {
    let client = ApiClient::new(settings)?;
    let (method, path, body) = match args {
        [action, flag_project, project_id, flag_deliveries, directory]
            if action == "create" && flag_project == "--project" && flag_deliveries == "--deliveries" =>
            (Method::POST, "/api/v1/runs".to_owned(), Some(json!({"project_id":project_id,"delivery_directory":directory}))),
        [action, id] if action == "status" => (Method::GET, format!("/api/v1/runs/{id}"), None),
        [action, id] if action == "expedient" => (Method::GET, format!("/api/v1/expedients/{id}"), None),
        [action, id] if action == "cancel" => (Method::POST, format!("/api/v1/runs/{id}/cancel"), Some(json!({}))),
        [action, id] if action == "cleanup" => (Method::POST, format!("/api/v1/expedients/{id}/cleanup"), Some(json!({}))),
        _ => return Err("usage: thot run create --project ID --deliveries DIR | status RUN_ID | expedient EXPEDIENT_ID | cancel RUN_ID | cleanup EXPEDIENT_ID".into()),
    };
    let value: Value = client.request(method, &path, body).await?;
    println!("{}", serde_json::to_string_pretty(&value)?);
    Ok(())
}
