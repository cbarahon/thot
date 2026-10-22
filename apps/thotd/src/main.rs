use std::{env, net::SocketAddr};
use thot_engine::{SqliteStudentRepository, StudentService};
use tracing_subscriber::EnvFilter;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .init();

    let host = env::var("THOTD_HOST").unwrap_or_else(|_| "127.0.0.1".into());
    let port: u16 = env::var("THOTD_PORT")
        .unwrap_or_else(|_| "8765".into())
        .parse()?;
    let database =
        env::var("THOTD_DATABASE").unwrap_or_else(|_| "sqlite:./data/thot.sqlite3".into());

    let repository = SqliteStudentRepository::connect(&database).await?;
    let service = StudentService::new(repository);
    let app = thot_api::router(service);
    let address: SocketAddr = format!("{host}:{port}").parse()?;
    let listener = tokio::net::TcpListener::bind(address).await?;

    tracing::info!(%address, %database, "thotd iniciado");
    axum_serve(listener, app).await?;
    Ok(())
}

async fn axum_serve(
    listener: tokio::net::TcpListener,
    app: axum::Router,
) -> Result<(), std::io::Error> {
    axum::serve(listener, app).await
}
