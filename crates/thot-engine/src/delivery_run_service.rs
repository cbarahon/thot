use serde::{Deserialize, Serialize};
use sqlx::{SqlitePool, sqlite::SqlitePoolOptions};
use std::{
    path::{Path, PathBuf},
    sync::Arc,
};
use tokio::sync::Mutex;

use crate::vm_activity::{self, DeliveryInput, ProjectSnapshot, VmActivityRuntime};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RunRecord {
    pub run_id: String,
    pub project_id: String,
    pub delivery_directory: String,
    pub status: String,
    pub expedient_ids: Vec<String>,
    pub error: Option<String>,
}

#[derive(Clone)]
pub struct DeliveryRunService {
    pool: SqlitePool,
    project_root: PathBuf,
    delivery_root: PathBuf,
    artifact_root: PathBuf,
    runtime: Arc<VmActivityRuntime>,
    create_gate: Arc<Mutex<()>>,
}

impl DeliveryRunService {
    pub async fn connect(
        database_url: &str,
        project_root: PathBuf,
        delivery_root: PathBuf,
        artifact_root: PathBuf,
        runtime: VmActivityRuntime,
    ) -> Result<Self, sqlx::Error> {
        let pool = SqlitePoolOptions::new()
            .max_connections(5)
            .connect(database_url)
            .await?;
        sqlx::query("CREATE TABLE IF NOT EXISTS vm_activity_runs (run_id TEXT PRIMARY KEY, project_id TEXT NOT NULL, delivery_directory TEXT NOT NULL, status TEXT NOT NULL, expedient_ids TEXT NOT NULL DEFAULT '[]', error TEXT, created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP)")
            .execute(&pool).await?;
        sqlx::query("CREATE TABLE IF NOT EXISTS vm_activity_deliveries (delivery_id TEXT PRIMARY KEY, collection_ref TEXT NOT NULL, source_ref TEXT NOT NULL, content_sha256 TEXT NOT NULL, original_filename TEXT NOT NULL, byte_size INTEGER NOT NULL, UNIQUE(collection_ref, source_ref, content_sha256))")
            .execute(&pool).await?;
        sqlx::query("CREATE TABLE IF NOT EXISTS vm_activity_expedients (expedient_id TEXT PRIMARY KEY, run_id TEXT NOT NULL, delivery_id TEXT NOT NULL, report_path TEXT NOT NULL, status TEXT NOT NULL, FOREIGN KEY(run_id) REFERENCES vm_activity_runs(run_id), FOREIGN KEY(delivery_id) REFERENCES vm_activity_deliveries(delivery_id))")
            .execute(&pool).await?;
        sqlx::query("UPDATE vm_activity_runs SET status='interrupted_cleanup_pending' WHERE status IN ('queued','running')").execute(&pool).await?;
        sqlx::query("UPDATE vm_activity_expedients SET status='interrupted_cleanup_pending' WHERE status='running'").execute(&pool).await?;
        Ok(Self {
            pool,
            project_root,
            delivery_root,
            artifact_root,
            runtime: Arc::new(runtime),
            create_gate: Arc::new(Mutex::new(())),
        })
    }

    pub async fn create_run(
        &self,
        project_id: String,
        delivery_directory: String,
    ) -> Result<RunRecord, ServiceError> {
        if !valid_id(&project_id) {
            return Err(ServiceError::Invalid(
                "project_id must be a simple project identifier".into(),
            ));
        }
        let root = self
            .delivery_root
            .canonicalize()
            .map_err(|e| ServiceError::Invalid(format!("delivery root is unavailable: {e}")))?;
        let selected = Path::new(&delivery_directory).canonicalize().map_err(|e| {
            ServiceError::Invalid(format!("delivery directory is unavailable: {e}"))
        })?;
        if !selected.is_dir() || !selected.starts_with(&root) {
            return Err(ServiceError::Invalid(
                "delivery directory must be inside THOTD_DELIVERY_ROOT".into(),
            ));
        }
        if std::fs::symlink_metadata(&delivery_directory).is_ok_and(|m| m.file_type().is_symlink())
        {
            return Err(ServiceError::Invalid(
                "delivery directory cannot be a symlink".into(),
            ));
        }

        let _guard = self.create_gate.lock().await;
        let active: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM vm_activity_runs WHERE status IN ('queued','running')",
        )
        .fetch_one(&self.pool)
        .await?;
        if active > 0 {
            return Err(ServiceError::Conflict(
                "another delivery run is queued or running".into(),
            ));
        }
        let run_id = uuid::Uuid::new_v4().to_string();
        let snapshot_dir = self
            .artifact_root
            .join(&project_id)
            .join(&run_id)
            .join("configuration");
        let snapshot = vm_activity::load_project(&self.project_root, &project_id, &snapshot_dir)
            .await
            .map_err(ServiceError::Invalid)?;
        let vbox_version = vm_activity::preflight(&self.runtime)
            .await
            .map_err(ServiceError::Invalid)?;
        let inputs = vm_activity::discover_deliveries(&selected, self.runtime.max_ova_bytes)
            .map_err(ServiceError::Invalid)?;
        let record = RunRecord {
            run_id: run_id.clone(),
            project_id,
            delivery_directory: selected.display().to_string(),
            status: "queued".into(),
            expedient_ids: Vec::new(),
            error: None,
        };
        sqlx::query("INSERT INTO vm_activity_runs(run_id,project_id,delivery_directory,status,expedient_ids) VALUES(?,?,?,?,?)")
            .bind(&record.run_id).bind(&record.project_id).bind(&record.delivery_directory).bind(&record.status).bind("[]").execute(&self.pool).await?;
        let service = self.clone();
        let background_run_id = record.run_id.clone();
        tokio::spawn(async move {
            service
                .execute_run(background_run_id, snapshot, vbox_version, inputs)
                .await;
        });
        Ok(record)
    }

    async fn execute_run(
        &self,
        run_id: String,
        project: ProjectSnapshot,
        vbox_version: String,
        inputs: Vec<DeliveryInput>,
    ) {
        let _ = sqlx::query(
            "UPDATE vm_activity_runs SET status='running' WHERE run_id=? AND status='queued'",
        )
        .bind(&run_id)
        .execute(&self.pool)
        .await;
        let mut expedients = Vec::new();
        let mut had_errors = false;
        if inputs.is_empty() {
            had_errors = true;
        }
        for input in inputs {
            if let Ok(Some(run)) = self.get_run(&run_id).await {
                if run.status == "cancelled" {
                    break;
                }
            }
            let mut input = input;
            if input.validation_error.is_none() {
                match vm_activity::retain_source(&self.runtime, &input).await {
                    Ok(source) => input.source = source,
                    Err(e) => input.validation_error = Some(e),
                }
            }
            let delivery_id = self
                .register_delivery(&input)
                .await
                .unwrap_or_else(|_| input.delivery_id.clone());
            let expedient_id = format!("exp-{}", uuid::Uuid::new_v4().simple());
            let report_dir = self
                .artifact_root
                .join(&project.project_id)
                .join(&run_id)
                .join(&delivery_id);
            let report_path = report_dir.join("delivery.json");
            let _ = sqlx::query("INSERT OR REPLACE INTO vm_activity_expedients(expedient_id,run_id,delivery_id,report_path,status) VALUES(?,?,?,?,?)")
                .bind(&expedient_id).bind(&run_id).bind(&delivery_id).bind(report_path.display().to_string()).bind("running").execute(&self.pool).await;
            expedients.push(expedient_id.clone());
            let _ = self.save_run_expedients(&run_id, &expedients).await;
            input.delivery_id = delivery_id;
            let report = vm_activity::execute_delivery(
                &self.runtime,
                &project,
                &vbox_version,
                &run_id,
                &expedient_id,
                &input,
                &report_dir,
                &report_path,
            )
            .await;
            let status = report["overall_status"]
                .as_str()
                .unwrap_or("completed_with_errors");
            if status != "completed" {
                had_errors = true;
            }
            let _ = sqlx::query("UPDATE vm_activity_expedients SET status=? WHERE expedient_id=?")
                .bind(status)
                .bind(&expedient_id)
                .execute(&self.pool)
                .await;
        }
        let final_status = if self
            .get_run(&run_id)
            .await
            .ok()
            .flatten()
            .is_some_and(|r| r.status == "cancelled")
        {
            "cancelled"
        } else if had_errors {
            "completed_with_errors"
        } else {
            "completed"
        };
        let _ = sqlx::query(
            "UPDATE vm_activity_runs SET status=? WHERE run_id=? AND status!='cancelled'",
        )
        .bind(final_status)
        .bind(run_id)
        .execute(&self.pool)
        .await;
    }

    async fn register_delivery(&self, input: &DeliveryInput) -> Result<String, sqlx::Error> {
        let collection_ref = input
            .original_source
            .parent()
            .unwrap_or(Path::new(""))
            .display()
            .to_string();
        let source_ref = input.original_source.display().to_string();
        let content_sha256 = input.sha256.clone().unwrap_or_default();
        sqlx::query("INSERT OR IGNORE INTO vm_activity_deliveries(delivery_id,collection_ref,source_ref,content_sha256,original_filename,byte_size) VALUES(?,?,?,?,?,?)")
            .bind(&input.delivery_id).bind(&collection_ref).bind(&source_ref).bind(&content_sha256).bind(&input.filename).bind(input.size as i64).execute(&self.pool).await?;
        sqlx::query_scalar("SELECT delivery_id FROM vm_activity_deliveries WHERE collection_ref=? AND source_ref=? AND content_sha256=?")
            .bind(collection_ref).bind(source_ref).bind(content_sha256).fetch_one(&self.pool).await
    }

    async fn save_run_expedients(&self, run_id: &str, ids: &[String]) -> Result<(), sqlx::Error> {
        let data = serde_json::to_string(ids).unwrap_or_else(|_| "[]".into());
        sqlx::query("UPDATE vm_activity_runs SET expedient_ids=? WHERE run_id=?")
            .bind(data)
            .bind(run_id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    pub async fn get_run(&self, run_id: &str) -> Result<Option<RunRecord>, ServiceError> {
        let row = sqlx::query_as::<_, (String,String,String,String,String,Option<String>)>("SELECT run_id, project_id, delivery_directory, status, expedient_ids, error FROM vm_activity_runs WHERE run_id = ?")
            .bind(run_id).fetch_optional(&self.pool).await?;
        Ok(row.map(
            |(run_id, project_id, delivery_directory, status, ids, error)| RunRecord {
                run_id,
                project_id,
                delivery_directory,
                status,
                expedient_ids: serde_json::from_str(&ids).unwrap_or_default(),
                error,
            },
        ))
    }

    pub async fn get_expedient(
        &self,
        expedient_id: &str,
    ) -> Result<Option<serde_json::Value>, ServiceError> {
        let path = sqlx::query_scalar::<_, String>(
            "SELECT report_path FROM vm_activity_expedients WHERE expedient_id = ?",
        )
        .bind(expedient_id)
        .fetch_optional(&self.pool)
        .await?;
        let Some(path) = path else { return Ok(None) };
        match tokio::fs::read(path).await {
            Ok(bytes) => serde_json::from_slice(&bytes)
                .map(Some)
                .map_err(|e| ServiceError::Internal(e.to_string())),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(ServiceError::Internal(e.to_string())),
        }
    }

    pub async fn cancel_run(&self, run_id: &str) -> Result<Option<RunRecord>, ServiceError> {
        let Some(mut run) = self.get_run(run_id).await? else {
            return Ok(None);
        };
        if matches!(run.status.as_str(), "queued" | "running") {
            run.status = "cancelled".into();
            sqlx::query("UPDATE vm_activity_runs SET status = ? WHERE run_id = ?")
                .bind(&run.status)
                .bind(run_id)
                .execute(&self.pool)
                .await?;
        }
        Ok(Some(run))
    }

    pub async fn cleanup_expedient(
        &self,
        expedient_id: &str,
    ) -> Result<Option<serde_json::Value>, ServiceError> {
        let row = sqlx::query_as::<_, (String, String)>(
            "SELECT report_path,delivery_id FROM vm_activity_expedients WHERE expedient_id=?",
        )
        .bind(expedient_id)
        .fetch_optional(&self.pool)
        .await?;
        let Some((path, delivery_id)) = row else {
            return Ok(None);
        };
        let mut report: serde_json::Value = match tokio::fs::read(&path).await {
            Ok(b) => {
                serde_json::from_slice(&b).map_err(|e| ServiceError::Internal(e.to_string()))?
            }
            Err(_) => return Ok(None),
        };
        let result = vm_activity::cleanup_from_manifest(
            &self.runtime,
            Path::new(&path).parent().unwrap_or(Path::new(".")),
            &delivery_id,
        )
        .await;
        report["cleanup"] = result;
        let cleanup_status = report["cleanup"]["status"]
            .as_str()
            .unwrap_or("failed")
            .to_owned();
        let cleanup_observation = report["cleanup"].clone();
        if let Some(results) = report["test_results"].as_array_mut() {
            if let Some(cleanup) = results.iter_mut().find(|r| r["test_id"] == "CLEAN-001") {
                let now = chrono::Utc::now().to_rfc3339();
                if cleanup["start_time"].is_null() {
                    cleanup["start_time"] = serde_json::json!(now);
                }
                cleanup["end_time"] = serde_json::json!(now);
                cleanup["status"] = serde_json::json!(cleanup_status);
                cleanup["observation"] = cleanup_observation.clone();
                cleanup["evidence"] = serde_json::json!(["cleanup-manifest.json"]);
                cleanup["diagnostics"] = if cleanup_status == "passed" {
                    serde_json::Value::Null
                } else {
                    serde_json::json!(cleanup_observation["details"].to_string())
                };
            }
        }
        report["finished_at"] = serde_json::json!(chrono::Utc::now().to_rfc3339());
        let all_passed = report["test_results"]
            .as_array()
            .is_some_and(|results| results.iter().all(|r| r["status"] == "passed"));
        report["overall_status"] = serde_json::json!(if all_passed {
            "completed"
        } else {
            "completed_with_errors"
        });
        let bytes = serde_json::to_vec_pretty(&report)
            .map_err(|e| ServiceError::Internal(e.to_string()))?;
        tokio::fs::write(&path, bytes)
            .await
            .map_err(|e| ServiceError::Internal(e.to_string()))?;
        let status = report["overall_status"]
            .as_str()
            .unwrap_or("completed_with_errors")
            .to_owned();
        sqlx::query("UPDATE vm_activity_expedients SET status=? WHERE expedient_id=?")
            .bind(&status)
            .bind(expedient_id)
            .execute(&self.pool)
            .await?;
        if report["cleanup"]["status"] == "passed" {
            let run_id = sqlx::query_scalar::<_, String>(
                "SELECT run_id FROM vm_activity_expedients WHERE expedient_id=?",
            )
            .bind(expedient_id)
            .fetch_one(&self.pool)
            .await?;
            let pending:i64=sqlx::query_scalar("SELECT COUNT(*) FROM vm_activity_expedients WHERE run_id=? AND status='interrupted_cleanup_pending'").bind(&run_id).fetch_one(&self.pool).await?;
            if pending == 0 {
                sqlx::query("UPDATE vm_activity_runs SET status='completed_with_errors' WHERE run_id=? AND status='interrupted_cleanup_pending'").bind(run_id).execute(&self.pool).await?;
            }
        }
        Ok(Some(report["cleanup"].clone()))
    }
}

fn valid_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 64
        && id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
}

#[derive(Debug, thiserror::Error)]
pub enum ServiceError {
    #[error("invalid request: {0}")]
    Invalid(String),
    #[error("conflict: {0}")]
    Conflict(String),
    #[error("service error: {0}")]
    Internal(String),
    #[error(transparent)]
    Storage(#[from] sqlx::Error),
}
