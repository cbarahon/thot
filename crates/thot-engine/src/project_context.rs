//! Purpose: Persists and resolves the server's current project for project-scoped workflows.
use serde::Serialize;
use sqlx::SqlitePool;
use std::path::{Path, PathBuf};
use thiserror::Error;

#[derive(Debug, Clone, Serialize)]
pub struct ProjectContext {
    pub project_id: String,
    pub project_name: String,
    pub project_path: String,
}

#[derive(Debug, Error)]
pub enum ProjectContextError {
    #[error("{0}")]
    Invalid(String),
    #[error("project context storage error: {0}")]
    Storage(#[from] sqlx::Error),
}

pub async fn ensure_schema(pool: &SqlitePool) -> Result<(), sqlx::Error> {
    sqlx::query(
        "CREATE TABLE IF NOT EXISTS thot_projects (project_id TEXT PRIMARY KEY, project_name TEXT NOT NULL, project_path TEXT NOT NULL, config_sha256 TEXT, updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP)",
    )
    .execute(pool)
    .await?;
    sqlx::query(
        "CREATE TABLE IF NOT EXISTS thot_runtime_context (context_key TEXT PRIMARY KEY, context_value TEXT NOT NULL, updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP)",
    )
    .execute(pool)
    .await?;
    sqlx::query(
        "CREATE TABLE IF NOT EXISTS project_students (project_id TEXT NOT NULL, student_id INTEGER NOT NULL, name TEXT NOT NULL, surname TEXT, email TEXT NOT NULL, external_id TEXT NOT NULL, group_name TEXT, center TEXT, birth_date TEXT, PRIMARY KEY(project_id, student_id), FOREIGN KEY(project_id) REFERENCES thot_projects(project_id) ON DELETE CASCADE, FOREIGN KEY(student_id) REFERENCES students(id) ON DELETE CASCADE)",
    )
    .execute(pool)
    .await?;
    sqlx::query(
        "CREATE TABLE IF NOT EXISTS project_assignments (project_id TEXT NOT NULL, student_id INTEGER NOT NULL, delivery_id TEXT NOT NULL, match_score REAL, PRIMARY KEY(project_id, student_id), FOREIGN KEY(project_id, student_id) REFERENCES project_students(project_id, student_id) ON DELETE CASCADE, FOREIGN KEY(delivery_id) REFERENCES vm_activity_deliveries(delivery_id) ON DELETE CASCADE)",
    )
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn activate(
    pool: &SqlitePool,
    project_id: &str,
    project_name: &str,
    project_path: &Path,
    config_sha256: Option<&str>,
) -> Result<ProjectContext, ProjectContextError> {
    ensure_schema(pool).await?;
    let canonical = project_path.canonicalize().map_err(|error| {
        ProjectContextError::Invalid(format!("project path is unavailable: {error}"))
    })?;
    let project_path = canonical.to_string_lossy().into_owned();
    sqlx::query("INSERT INTO thot_projects(project_id, project_name, project_path, config_sha256, updated_at) VALUES(?, ?, ?, ?, CURRENT_TIMESTAMP) ON CONFLICT(project_id) DO UPDATE SET project_name=excluded.project_name, project_path=excluded.project_path, config_sha256=COALESCE(excluded.config_sha256, thot_projects.config_sha256), updated_at=CURRENT_TIMESTAMP")
        .bind(project_id)
        .bind(project_name)
        .bind(&project_path)
        .bind(config_sha256)
        .execute(pool)
        .await?;
    sqlx::query("INSERT INTO thot_runtime_context(context_key, context_value, updated_at) VALUES('current_project_id', ?, CURRENT_TIMESTAMP) ON CONFLICT(context_key) DO UPDATE SET context_value=excluded.context_value, updated_at=CURRENT_TIMESTAMP")
        .bind(project_id)
        .execute(pool)
        .await?;
    Ok(ProjectContext {
        project_id: project_id.to_owned(),
        project_name: project_name.to_owned(),
        project_path,
    })
}

pub async fn resolve(
    pool: &SqlitePool,
    requested_path: Option<&str>,
) -> Result<ProjectContext, ProjectContextError> {
    ensure_schema(pool).await?;
    if let Some(requested_path) = requested_path {
        let source = PathBuf::from(requested_path);
        let metadata = std::fs::symlink_metadata(&source).map_err(|error| {
            ProjectContextError::Invalid(format!(
                "project source {} is unavailable: {error}",
                source.display()
            ))
        })?;
        if metadata.file_type().is_symlink() {
            return Err(ProjectContextError::Invalid(
                "project source must not be a symlink".into(),
            ));
        }
        let canonical = source.canonicalize().map_err(|error| {
            ProjectContextError::Invalid(format!("cannot resolve project path: {error}"))
        })?;
        let project_name = if metadata.is_dir() {
            let config_path = canonical.join("configuration.yaml");
            let config_metadata = std::fs::symlink_metadata(&config_path).map_err(|error| {
                ProjectContextError::Invalid(format!(
                    "project folder must contain configuration.yaml: {error}"
                ))
            })?;
            if config_metadata.file_type().is_symlink() || !config_metadata.is_file() {
                return Err(ProjectContextError::Invalid(
                    "project configuration.yaml must be a regular file".into(),
                ));
            }
            let bytes = std::fs::read(&config_path).map_err(|error| {
                ProjectContextError::Invalid(format!("cannot read project configuration: {error}"))
            })?;
            crate::project_config::parse_configuration(&bytes)
                .map_err(ProjectContextError::Invalid)?
                .activity_name
        } else if metadata.is_file() && supported_project_archive(&canonical) {
            let project_id = crate::vm_activity::project_identifier_for_path(&canonical)
                .map_err(ProjectContextError::Invalid)?;
            sqlx::query_scalar::<_, String>(
                "SELECT project_name FROM thot_projects WHERE project_id=?",
            )
            .bind(project_id)
            .fetch_optional(pool)
            .await?
            .unwrap_or_else(|| display_project_name(&canonical))
        } else {
            return Err(ProjectContextError::Invalid(
                "project path must be a project directory or supported .thot archive".into(),
            ));
        };
        let project_id = crate::vm_activity::project_identifier_for_path(&canonical)
            .map_err(ProjectContextError::Invalid)?;
        return activate(pool, &project_id, &project_name, &canonical, None).await;
    }

    let current = sqlx::query_as::<_, (String, String, String)>(
        "SELECT p.project_id, p.project_name, p.project_path FROM thot_runtime_context c JOIN thot_projects p ON p.project_id=c.context_value WHERE c.context_key='current_project_id'",
    )
    .fetch_optional(pool)
    .await?;
    current
        .map(|(project_id, project_name, project_path)| ProjectContext {
            project_id,
            project_name,
            project_path,
        })
        .ok_or_else(|| {
            ProjectContextError::Invalid(
                "no current project is selected; run `thot run create PROJECT` or use a project folder containing configuration.yaml".into(),
            )
        })
}

fn supported_project_archive(path: &Path) -> bool {
    path.file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| {
            [".thot.gz", ".thot.tgz", ".thot.zip"]
                .iter()
                .any(|suffix| name.ends_with(suffix))
        })
}

fn display_project_name(path: &Path) -> String {
    let filename = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("project");
    [".thot.tar.gz", ".thot.tgz", ".thot.zip", ".thot.gz"]
        .iter()
        .find_map(|suffix| filename.strip_suffix(suffix))
        .unwrap_or(filename)
        .to_owned()
}
