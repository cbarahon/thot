//! Application services and persistence for Thot.

pub mod delivery_run_service;
pub mod storage;
mod students;
pub mod vm_activity;
pub mod project_archive; // Export the project_archive module

pub use delivery_run_service::{DeliveryRunService, RunRecord};
pub use storage::SqliteStudentRepository;
pub use students::StudentService;

pub use project_archive::project_config; // Export project_config as a public engine module

use thiserror::Error;

#[derive(Debug, Error)]
pub enum EngineError {
    #[error("error de almacenamiento: {0}")]
    Storage(#[from] sqlx::Error),
    #[error("error de inicialización de almacenamiento: {0}")]
    StorageInit(String),
}
