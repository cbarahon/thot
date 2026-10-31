//! Purpose: Parses run and expedient CLI commands and calls the corresponding API routes.
use crate::{Settings, api_client::ApiClient, output::print_yaml};
use reqwest::Method;
use serde_json::{Value, json};
use std::path::Path;

pub async fn handle(
    args: &[String],
    settings: &Settings,
) -> Result<(), Box<dyn std::error::Error>> {
    let client = ApiClient::new(settings)?;
    if matches!(args, [action] if action == "status") {
        let runs: Value = client.request(Method::GET, "/api/v1/runs", None).await?;
        let active = runs
            .as_array()
            .into_iter()
            .flatten()
            .filter(|run| {
                matches!(
                    run.get("status").and_then(Value::as_str),
                    Some("queued" | "running" | "cancelling")
                )
            })
            .filter_map(|run| run.get("run_id").and_then(Value::as_str))
            .collect::<Vec<_>>();
        if active.len() != 1 {
            return Err(format!(
                "found {} active runs; specify a RUN_ID with `thot run status RUN_ID`",
                active.len()
            )
            .into());
        }
        let value: Value = client
            .request(Method::GET, &format!("/api/v1/runs/{}", active[0]), None)
            .await?;
        print_yaml(&value)?;
        return Ok(());
    }
    let (method, path, body) = match args {
        [command] if command == "list" => {
            (Method::GET, "/api/v1/runs".to_owned(), None)
        }
        [command] if command == "checks" => (
            Method::POST,
            "/api/v1/runs/checks".to_owned(),
            Some(json!({})),
        ),
        [] => (
            Method::POST,
            "/api/v1/runs".to_owned(),
            Some(json!({"project_path":discover_project()?})),
        ),
        [action, project_file] if action == "create" => (
            Method::POST,
            "/api/v1/runs".to_owned(),
            Some(json!({"project_path":absolute_path(project_file)?})),
        ),
        [action, flag_project, project_file]
            if action == "create" && flag_project == "--project" =>
        {
            (
                Method::POST,
                "/api/v1/runs".to_owned(),
                Some(json!({"project_path":absolute_path(project_file)?})),
            )
        }
        [action, project_file, flag_deliveries, directory]
            if action == "create" && flag_deliveries == "--deliveries" =>
        {
            let body = json!({
                "project_path": absolute_path(project_file)?,
                "delivery_directory": absolute_path(directory)?
            });
            (Method::POST, "/api/v1/runs".to_owned(), Some(body))
        }
        [action, flag_project, project_file, flag_deliveries, directory]
            if action == "create"
                && flag_project == "--project"
                && flag_deliveries == "--deliveries" =>
        {
            let body = json!({
                "project_path": absolute_path(project_file)?,
                "delivery_directory": absolute_path(directory)?
            });
            (Method::POST, "/api/v1/runs".to_owned(), Some(body))
        }
        [action, id] if action == "status" => (Method::GET, format!("/api/v1/runs/{id}"), None),
        [action, id] if action == "checks" => (
            Method::POST,
            "/api/v1/runs/checks".to_owned(),
            Some(json!({"run_id": id})),
        ),
        [action, id] if action == "delete" => {
            (Method::DELETE, format!("/api/v1/runs/{id}"), None)
        }
        [action, id] if action == "expedient" => (Method::GET, format!("/api/v1/expedients/{id}"), None),
        [action, id] if action == "cancel" => (Method::POST, format!("/api/v1/runs/{id}/cancel"), Some(json!({}))),
        [action, id] if action == "cleanup" => (Method::POST, format!("/api/v1/expedients/{id}/cleanup"), Some(json!({}))),
        _ => return Err("usage: thot run [list] | create <project-file> [--deliveries DIR] | status [RUN_ID] | checks [RUN_ID] | delete RUN_ID | expedient EXPEDIENT_ID | cancel RUN_ID | cleanup EXPEDIENT_ID".into()),
    };
    let value: Value = client.request(method, &path, body).await?;
    print_yaml(&value)?;
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

fn discover_project() -> Result<String, Box<dyn std::error::Error>> {
    let current_dir = std::env::current_dir()?;
    let mut sources = Vec::new();

    for entry in std::fs::read_dir(&current_dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.is_file()
            && [".thot.gz", ".thot.tgz", ".thot.zip"].iter().any(|suffix| {
                path.file_name()
                    .and_then(|name| name.to_str())
                    .is_some_and(|name| name.ends_with(suffix))
            })
        {
            sources.push(path);
        }
    }
    let configuration = current_dir.join("configuration.yaml");
    if configuration.is_file() {
        sources.push(configuration);
    }

    if sources.len() == 1 {
        let source = sources.pop().expect("one source was found");
        if source.file_name().and_then(|name| name.to_str()) == Some("configuration.yaml") {
            return Ok(current_dir.to_string_lossy().into_owned());
        }
        return Ok(source.to_string_lossy().into_owned());
    } else if sources.len() > 1 {
        return Err("multiple project sources found; use `thot run create PATH`".into());
    }

    Err("no project source found; use `thot run create PATH`".into())
}
