//! Purpose: Parses run and expedient CLI commands and calls the corresponding API routes.
use crate::{Settings, api_client::ApiClient, current_project_path, output::print_yaml};
use reqwest::Method;
use serde_json::{Value, json};
use std::path::Path;

pub(crate) async fn select_run(
    client: &ApiClient,
    id: Option<&String>,
) -> Result<String, Box<dyn std::error::Error>> {
    if let Some(id) = id {
        return Ok(id.clone());
    }
    let runs: Value = client.request(Method::GET, "/api/v1/runs", None).await?;
    let project = current_project_path()?
        .map(|path| thot_engine::vm_activity::project_identifier_for_path(Path::new(&path)))
        .transpose()?;
    runs.as_array()
        .into_iter()
        .flatten()
        .find(|r| {
            project
                .as_ref()
                .is_none_or(|p| r["project_id"].as_str() == Some(p))
        })
        .and_then(|r| r["run_id"].as_str())
        .map(str::to_owned)
        .ok_or_else(|| "no evaluation found for this project".into())
}

pub async fn handle(
    args: &[String],
    settings: &Settings,
) -> Result<(), Box<dyn std::error::Error>> {
    let client = ApiClient::new(settings)?;
    if matches!(args.first().map(String::as_str), Some("summary" | "log")) && args.len() <= 2 {
        let id = select_run(&client, args.get(1)).await?;
        let run: Value = client
            .request(Method::GET, &format!("/api/v1/runs/{id}"), None)
            .await?;
        if args[0] == "summary" {
            println!("Run {id}   {}", run["status"].as_str().unwrap_or("unknown"));
            if let Some(e) = run["error"].as_str() {
                println!("{e}");
            }
            for row in run["expedients"].as_array().into_iter().flatten() {
                println!(
                    "{}   {}",
                    row["student_name"].as_str().unwrap_or("Student"),
                    row["status"].as_str().unwrap_or("unknown")
                );
            }
        } else {
            print_yaml(&run)?;
            for exp in run["expedient_ids"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(Value::as_str)
            {
                let details: Value = client
                    .request(Method::GET, &format!("/api/v1/expedients/{exp}"), None)
                    .await?;
                print_yaml(&details)?;
            }
        }
        return Ok(());
    }
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
            Some(json!({ "project_path": current_project_path()? })),
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
            Some(json!({
                "run_id": id,
                "project_path": current_project_path()?
            })),
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
    if path == "/api/v1/runs" && args.first().is_some_and(|s| s == "create") {
        println!(
            "Started project {} in the background\nRun {}   {}",
            value["project_id"].as_str().unwrap_or("unknown"),
            value["run_id"].as_str().unwrap_or("unknown"),
            value["status"].as_str().unwrap_or("queued")
        );
        for warning in value["warnings"].as_array().into_iter().flatten() {
            println!("{}", warning.as_str().unwrap_or_default());
        }
    } else {
        print_yaml(&value)?;
    }
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

pub(crate) fn discover_project() -> Result<String, Box<dyn std::error::Error>> {
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
    let canonical_configuration = current_dir.join("configuration.thot.yaml");
    let legacy_configuration = current_dir.join("configuration.yaml");
    if canonical_configuration.is_file() && legacy_configuration.is_file() {
        return Err("project contains both configuration.thot.yaml and configuration.yaml".into());
    }
    let configuration = if canonical_configuration.is_file() {
        canonical_configuration
    } else {
        legacy_configuration
    };
    if configuration.is_file() {
        sources.push(configuration);
    }

    if sources.len() == 1 {
        let source = sources.pop().expect("one source was found");
        if source
            .file_name()
            .and_then(|name| name.to_str())
            .is_some_and(|name| matches!(name, "configuration.yaml" | "configuration.thot.yaml"))
        {
            return Ok(current_dir.to_string_lossy().into_owned());
        }
        return Ok(source.to_string_lossy().into_owned());
    } else if sources.len() > 1 {
        return Err("multiple project sources found; use `thot run create PATH`".into());
    }

    Err("no project source found; use `thot run create PATH`".into())
}
