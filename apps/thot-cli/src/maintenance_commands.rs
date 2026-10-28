//! Purpose: Runs CLI cleanup, prune, and reset commands with destructive-action confirmations.
use crate::{Settings, api_client::ApiClient, output::print_yaml};
use reqwest::Method;
use serde_json::Value;
use std::io::{self, IsTerminal, Write};
use uuid::Uuid;

pub async fn handle(command: &str, settings: &Settings) -> Result<(), Box<dyn std::error::Error>> {
    let endpoint = match command {
        "clean" => "/api/v1/maintenance/clean",
        "prune" => {
            if !confirm_prune()? {
                return Ok(());
            }
            "/api/v1/maintenance/prune"
        }
        "reset" => {
            if !confirm_reset()? {
                return Ok(());
            }
            "/api/v1/maintenance/reset"
        }
        _ => return Err(format!("unknown maintenance command: {command}").into()),
    };
    let response: Value = ApiClient::new(settings)?
        .request(Method::POST, endpoint, None)
        .await?;
    print_yaml(&response)?;
    Ok(())
}

fn confirm_prune() -> Result<bool, Box<dyn std::error::Error>> {
    if !io::stdin().is_terminal() {
        eprintln!("prune requires an interactive terminal; no data was changed");
        return Ok(false);
    }
    eprintln!(
        "WARNING: prune permanently deletes all but the latest run for each project, including associated Thot runtime data."
    );
    eprint!("Continue? Type y to confirm: ");
    io::stderr().flush()?;
    let mut answer = String::new();
    io::stdin().read_line(&mut answer)?;
    Ok(answer.trim() == "y")
}

fn confirm_reset() -> Result<bool, Box<dyn std::error::Error>> {
    if !io::stdin().is_terminal() {
        eprintln!("reset requires an interactive terminal; no data was changed");
        return Ok(false);
    }
    let challenge_source = Uuid::new_v4().simple().to_string();
    let challenge_value = u32::from_str_radix(&challenge_source[..8], 16)? % 1_000_000;
    let challenge = format!("{challenge_value:06}");
    eprintln!(
        "WARNING: reset permanently erases all Thot run, delivery, expedient, student, artifact, and VM runtime data. Installation configuration, project archives, and original delivery files are preserved."
    );
    eprintln!("To confirm, type this six-digit code exactly: {challenge}");
    eprint!("> ");
    io::stderr().flush()?;
    let mut answer = String::new();
    io::stdin().read_line(&mut answer)?;
    Ok(answer.trim() == challenge)
}
