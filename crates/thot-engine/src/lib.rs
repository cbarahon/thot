//! Casos de uso y almacenamiento. La capa HTTP no accede directamente a SQL.

pub mod storage;
mod students;

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
