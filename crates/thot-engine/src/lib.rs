//! Application services and persistence for Thot.

pub mod delivery_run_service;
pub mod storage;
mod students;
pub mod vm_activity;

pub use delivery_run_service::{DeliveryRunService, RunRecord};
pub use storage::SqliteStudentRepository;
pub use students::StudentService;

use thiserror::Error;

#[derive(Debug, Error)]
pub enum EngineError {
    #[error("error de almacenamiento: {0}")]
    Storage(#[from] sqlx::Error),
    #[error("error de inicialización de almacenamiento: {0}")]
    StorageInit(String),
}
