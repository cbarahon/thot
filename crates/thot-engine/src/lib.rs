//! Purpose: Exports the engine's project, test, student, and delivery-run services.

pub mod assignments;
pub mod delivery_executor;
pub mod delivery_run_service;
pub mod maintenance;
pub mod maintenance_plan;
pub mod maintenance_vms;
pub mod project_archive;
pub mod project_config;
pub mod storage;
mod students;
pub mod test_execution;
pub mod test_executor;
pub mod test_result_store;
pub mod tests;
pub mod vm_activity;

pub use assignments::AssignmentService;
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
