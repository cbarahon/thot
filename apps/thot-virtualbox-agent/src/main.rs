mod agent;
mod config;

use axum::{
    Router,
    routing::{get, post},
};
use std::{env, path::PathBuf, sync::Arc};
use tokio::sync::{Mutex, RwLock};
use tracing_subscriber::EnvFilter;

#[derive(Clone)]
struct AppState {
    config: Arc<config::Config>,
    tasks: Arc<RwLock<std::collections::HashMap<String, agent::TaskEntry>>>,
    vm_gate: Arc<Mutex<()>>,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .init();
    let config_path = env::args_os()
        .nth(1)
        .map(PathBuf::from)
        .or_else(|| env::var_os("THOT_VIRTUALBOX_AGENT_CONFIG").map(PathBuf::from))
        .unwrap_or_else(|| PathBuf::from("thot-virtualbox-agent.config.yaml"));
    let config = Arc::new(config::Config::load(&config_path).map_err(std::io::Error::other)?);
    let public_key = config.ensure_key_pair().map_err(std::io::Error::other)?;
    tracing::info!(path = %config_path.display(), "VirtualBox agent loaded configuration");
    tracing::info!(
        "Add this VirtualBox agent public key to the configured SSH user's authorized_keys:\n{}",
        public_key.trim()
    );
    let state = AppState {
        config: config.clone(),
        tasks: Arc::new(RwLock::new(std::collections::HashMap::new())),
        vm_gate: Arc::new(Mutex::new(())),
    };
    let app = Router::new()
        .route("/health", get(agent::health))
        .route("/v1/capabilities", get(agent::capabilities))
        .route("/v1/tasks", post(agent::submit))
        .route(
            "/v1/tasks/{task_id}",
            get(agent::task_status).delete(agent::cancel_task),
        )
        .with_state(state);
    let address = config.socket_addr().map_err(std::io::Error::other)?;
    let listener = tokio::net::TcpListener::bind(address).await?;
    tracing::info!(%address, "thot-virtualbox-agent started");
    axum::serve(listener, app).await?;
    Ok(())
}
