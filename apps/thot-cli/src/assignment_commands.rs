//! Purpose: Parses assignment CLI commands, reads YAML/JSON imports, and renders responses.
use crate::{
    Settings, api_client::ApiClient, current_project_path, output::print_yaml, with_project_query,
};
use reqwest::Method;
use serde_json::{Value, json};
use std::{fs, path::Path};

pub async fn handle(
    args: &[String],
    settings: &Settings,
) -> Result<(), Box<dyn std::error::Error>> {
    let (action, input_path, json_output) = parse_args(args)?;
    let client = ApiClient::new(settings)?;
    let project_path = current_project_path()?;
    let response = match action {
        "guess" => {
            client
                .request(
                    Method::POST,
                    "/api/v1/assignments/guess",
                    Some(json!({"project_path": project_path})),
                )
                .await?
        }
        "get" => {
            let endpoint = with_project_query("/api/v1/assignments/get", project_path.as_deref());
            client.request(Method::GET, &endpoint, None).await?
        }
        "import" => {
            let path = input_path.ok_or("usage: thot assignments import FILE [--json]")?;
            let mut value = read_input(Path::new(&path))?;
            if let Some(project_path) = project_path {
                value["project_path"] = json!(project_path);
            }
            client
                .request(Method::POST, "/api/v1/assignments/import", Some(value))
                .await?
        }
        _ => return Err("usage: thot assignments guess|get|import FILE [--json]".into()),
    };
    if json_output {
        println!("{}", serde_json::to_string_pretty(&response)?);
    } else {
        print_yaml(&response)?;
    }
    Ok(())
}

fn parse_args(args: &[String]) -> Result<(&str, Option<String>, bool), Box<dyn std::error::Error>> {
    let Some(action) = args.first().map(String::as_str) else {
        return Err("usage: thot assignments guess|get|import FILE [--json]".into());
    };
    let mut json_output = false;
    let mut positional = Vec::new();
    for argument in &args[1..] {
        if argument == "--json" {
            if json_output {
                return Err("--json may only be specified once".into());
            }
            json_output = true;
        } else {
            positional.push(argument.clone());
        }
    }
    let input_path = match (action, positional.as_slice()) {
        ("guess" | "get", []) => None,
        ("import", [path]) => Some(path.clone()),
        ("guess" | "get", _) => {
            return Err("usage: thot assignments guess|get [--json]".into());
        }
        ("import", _) => return Err("usage: thot assignments import FILE [--json]".into()),
        _ => return Err("usage: thot assignments guess|get|import FILE [--json]".into()),
    };
    Ok((action, input_path, json_output))
}

fn read_input(path: &Path) -> Result<Value, Box<dyn std::error::Error>> {
    let contents = fs::read_to_string(path)?;
    let value = match serde_json::from_str::<Value>(&contents) {
        Ok(value) => value,
        Err(json_error) => {
            let yaml =
                serde_yaml::from_str::<serde_yaml::Value>(&contents).map_err(|yaml_error| {
                    format!("invalid JSON ({json_error}) or YAML ({yaml_error})")
                })?;
            serde_json::to_value(yaml)?
        }
    };
    if !value.is_object() || !value.get("assignments").is_some_and(Value::is_array) {
        return Err("assignment input must be an object with an assignments array".into());
    }
    Ok(value)
}
