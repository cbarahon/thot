//! Purpose: Daemon entry point that loads runtime settings and starts the Thot API service.
mod vm_activity_config;

use std::{env, net::SocketAddr};
use thot_engine::{
    DeliveryRunService, SqliteStudentRepository, StudentService, vm_activity::VmActivityRuntime,
};
use tracing_subscriber::EnvFilter;
use vm_activity_config::VmActivityConfig;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .init();

    let config = VmActivityConfig::from_env().map_err(std::io::Error::other)?;
    for root in [
        &config.artifact_root,
        &config.storage_root,
        &config.vbox_user_home,
    ] {
        std::fs::create_dir_all(root)?;
    }
    let database = env::var("THOTD_DATABASE").unwrap_or_else(|_| config.database_url.clone());
    let student_repository = SqliteStudentRepository::connect(&database).await?;
    let runtime = VmActivityRuntime {
        vbox_manage: config.vbox_manage_path.clone(),
        vbox_user_home: config.vbox_user_home.clone(),
        expected_vbox_version: config.expected_vbox_version.clone(),
        storage_root: config.storage_root.clone(),
        artifact_root: config.artifact_root.clone(),
        max_ova_bytes: config.ova_size_limit,
        free_space_reserve: config.free_space_reserve,
        output_limit: config.output_limit,
        import_timeout: config.import_timeout,
        startup_timeout: config.startup_timeout,
        ssh_timeout: config.ssh_timeout,
        shutdown_grace: config.shutdown_grace,
        ssh_secret_file: config.ssh_secret_source.clone(),
        firewall_confirmed: config.firewall_confirmation,
    };
    let run_service =
        DeliveryRunService::connect(&database, config.artifact_root.clone(), runtime).await?;

    let app = thot_api::router_with_runs(StudentService::new(student_repository), run_service);
    let bind_ip = config.bind_host.parse()?;
    let address = SocketAddr::new(bind_ip, config.bind_port);
    let listener = tokio::net::TcpListener::bind(address).await?;
    tracing::info!(%address, %database, "thotd started");
    axum::serve(listener, app).await?;
    Ok(())
}
