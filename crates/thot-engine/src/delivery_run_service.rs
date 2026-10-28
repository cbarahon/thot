//! Purpose: Coordinates delivery runs, database state, delivery discovery, and expedient cleanup.
use futures_util::stream::{self, StreamExt};
use serde::{Deserialize, Serialize};
use sqlx::{SqlitePool, sqlite::SqlitePoolOptions};
use std::{
    fs,
    path::{Path, PathBuf},
    sync::Arc,
};
use tokio::sync::Mutex;

use crate::{
    delivery_executor,
    vm_activity::{self, DeliveryInput, ProjectSnapshot, VmActivityRuntime},
};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RunRecord {
    pub run_id: String,
    pub project_id: String,
    pub created_at: String,
    pub delivery_directory: String,
    pub status: String,
    pub expedient_ids: Vec<String>,
    pub error: Option<String>,
}

#[derive(Clone)]
pub struct DeliveryRunService {
    pool: SqlitePool,
    artifact_root: PathBuf,
    runtime: Arc<VmActivityRuntime>,
    create_gate: Arc<Mutex<()>>,
}

impl DeliveryRunService {
    pub async fn connect(
        database_url: &str,
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
        sqlx::query("UPDATE vm_activity_runs SET status='interrupted_cleanup_pending' WHERE status IN ('queued','running','cancelling')").execute(&pool).await?;
        sqlx::query("UPDATE vm_activity_expedients SET status='interrupted_cleanup_pending' WHERE status='running'").execute(&pool).await?;
        Ok(Self {
            pool,
            artifact_root,
            runtime: Arc::new(runtime),
            create_gate: Arc::new(Mutex::new(())),
        })
    }

    pub async fn create_run(
        &self,
        project_path: String,
        delivery_directory: Option<String>,
    ) -> Result<RunRecord, ServiceError> {
        if !Path::new(&project_path).is_absolute() {
            return Err(ServiceError::Invalid(
                "project_path must be an absolute path to a project archive".into(),
            ));
        }
        let _guard = self.create_gate.lock().await;
        let active: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM vm_activity_runs WHERE status IN ('queued','running','cancelling')",
        )
        .fetch_one(&self.pool)
        .await?;
        if active > 0 {
            return Err(ServiceError::Conflict(
                "another delivery run is queued or running".into(),
            ));
        }
        let now = chrono::Utc::now();
        let created_at = now.to_rfc3339_opts(chrono::SecondsFormat::Secs, true);
        let timestamp = now.format("%Y%m%d_%H%M%S").to_string();
        let project_name = project_file_component(Path::new(&project_path));
        let base_run_id = format!("{project_name}-{timestamp}");
        let mut run_id = base_run_id.clone();
        let mut collision = 1_u32;
        while sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM vm_activity_runs WHERE run_id=?")
            .bind(&run_id)
            .fetch_one(&self.pool)
            .await?
            > 0
        {
            run_id = format!("{base_run_id}-{collision:02}");
            collision += 1;
        }
        let snapshot_dir = self.artifact_root.join("project-snapshots").join(&run_id);
        let snapshot = vm_activity::load_project(Path::new(&project_path), &snapshot_dir)
            .await
            .map_err(ServiceError::Invalid)?;
        let selected = if let Some(override_path) = delivery_directory {
            resolve_directory(Path::new(&override_path), None).map_err(ServiceError::Invalid)?
        } else {
            resolve_directory(
                Path::new(&snapshot.configuration.delivery_folder),
                Some(&snapshot.archive_parent),
            )
            .map_err(ServiceError::Invalid)?
        };
        let uses_vm_ports = snapshot.configuration.tests.iter().any(|test| {
            test.test_ref.starts_with("VirtualBox/") || test.test_ref == "ssh/get_host_info"
        });
        if uses_vm_ports && snapshot.configuration.concurrency > 1 {
            return Err(ServiceError::Invalid(
                "concurrency greater than one is not supported for projects using shared VirtualBox or SSH forwarding ports".into(),
            ));
        }
        let vbox_version = if uses_vm_ports {
            vm_activity::preflight(&self.runtime)
                .await
                .map_err(ServiceError::Invalid)?
        } else {
            "not used".to_owned()
        };
        let inputs = vm_activity::discover_deliveries(&selected, self.runtime.max_ova_bytes)
            .map_err(ServiceError::Invalid)?;
        let record = RunRecord {
            run_id: run_id.clone(),
            project_id: snapshot.project_id.clone(),
            created_at: created_at.clone(),
            delivery_directory: selected.display().to_string(),
            status: "queued".into(),
            expedient_ids: Vec::new(),
            error: None,
        };
        sqlx::query("INSERT INTO vm_activity_runs(run_id,project_id,delivery_directory,status,expedient_ids,created_at) VALUES(?,?,?,?,?,?)")
            .bind(&record.run_id).bind(&record.project_id).bind(&record.delivery_directory).bind(&record.status).bind("[]").bind(&record.created_at).execute(&self.pool).await?;
        let service = self.clone();
        let background_run_id = record.run_id.clone();
        let background_timestamp = timestamp;
        tokio::spawn(async move {
            service
                .execute_run(
                    background_run_id,
                    background_timestamp,
                    snapshot,
                    vbox_version,
                    inputs,
                )
                .await;
        });
        Ok(record)
    }

    async fn execute_run(
        &self,
        run_id: String,
        run_timestamp: String,
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
        struct DeliveryWork {
            expedient_id: String,
            input: DeliveryInput,
            report_dir: PathBuf,
            report_path: PathBuf,
        }
        let mut expedients = Vec::new();
        let mut work_items = Vec::new();
        let mut had_errors = inputs.is_empty();
        for mut input in inputs {
            if !self.run_accepts_work(&run_id).await {
                break;
            }
            if input.validation_error.is_none() {
                match vm_activity::retain_source(&self.runtime, &input).await {
                    Ok(source) => input.source = source,
                    Err(error) => input.validation_error = Some(error),
                }
            }
            if !self.run_accepts_work(&run_id).await {
                break;
            }
            let delivery_id = match self.register_delivery(&input).await {
                Ok(id) => id,
                Err(error) => {
                    tracing::error!(%error, delivery = %input.filename, "could not register delivery");
                    had_errors = true;
                    continue;
                }
            };
            let filename = Path::new(&input.filename)
                .file_name()
                .unwrap_or_default()
                .to_string_lossy();
            let expedient_base = format!("{}-{run_timestamp}", expedient_file_component(&filename));
            let mut expedient_id = expedient_base.clone();
            let mut collision = 1_u32;
            loop {
                match sqlx::query_scalar::<_, i64>(
                    "SELECT COUNT(*) FROM vm_activity_expedients WHERE expedient_id=?",
                )
                .bind(&expedient_id)
                .fetch_one(&self.pool)
                .await
                {
                    Ok(0) => break,
                    Ok(_) => {
                        expedient_id = format!("{expedient_base}-{collision:02}");
                        collision += 1;
                    }
                    Err(error) => {
                        tracing::error!(%error, "could not check expedient ID collision");
                        had_errors = true;
                        expedient_id = format!(
                            "{expedient_base}-{}",
                            &uuid::Uuid::new_v4().simple().to_string()[..8]
                        );
                        break;
                    }
                }
            }
            let report_dir = self
                .artifact_root
                .join(&project.project_id)
                .join(&run_id)
                .join(&delivery_id);
            let report_path = report_dir.join("delivery.json");
            if let Err(error) = sqlx::query("INSERT INTO vm_activity_expedients(expedient_id,run_id,delivery_id,report_path,status) VALUES(?,?,?,?,?)")
                .bind(&expedient_id).bind(&run_id).bind(&delivery_id).bind(report_path.display().to_string()).bind("queued").execute(&self.pool).await
            {
                tracing::error!(%error, expedient_id, "could not record delivery expedient");
                had_errors = true;
                continue;
            }
            expedients.push(expedient_id.clone());
            input.delivery_id = delivery_id;
            work_items.push(DeliveryWork {
                expedient_id,
                input,
                report_dir,
                report_path,
            });
        }
        if let Err(error) = self.save_run_expedients(&run_id, &expedients).await {
            tracing::error!(%error, run_id, "could not persist run expedient list");
            had_errors = true;
        }

        let concurrency = project.configuration.concurrency.max(1) as usize;
        let outcomes = stream::iter(work_items)
            .map(|work| {
                let service = self;
                let run_id = run_id.clone();
                let project = project.clone();
                let vbox_version = vbox_version.clone();
                async move {
                if !service.run_accepts_work(&run_id).await {
                    let _ = sqlx::query("UPDATE vm_activity_expedients SET status='cancelled' WHERE expedient_id=?")
                        .bind(&work.expedient_id).execute(&service.pool).await;
                    return false;
                }
                if let Err(error) = sqlx::query("UPDATE vm_activity_expedients SET status='running' WHERE expedient_id=?")
                    .bind(&work.expedient_id).execute(&service.pool).await
                {
                    tracing::error!(%error, expedient_id = %work.expedient_id, "could not mark expedient running");
                    return true;
                }
                let report = delivery_executor::execute_delivery(
                    &service.runtime,
                    &project,
                    &vbox_version,
                    &run_id,
                    &work.expedient_id,
                    &work.input,
                    &work.report_dir,
                    &work.report_path,
                ).await;
                let status = report["overall_status"].as_str().unwrap_or("completed_with_errors");
                if let Err(error) = sqlx::query("UPDATE vm_activity_expedients SET status=? WHERE expedient_id=?")
                    .bind(status).bind(&work.expedient_id).execute(&service.pool).await
                {
                    tracing::error!(%error, expedient_id = %work.expedient_id, "could not save expedient status");
                    return true;
                }
                status != "completed"
                }
            })
            .buffer_unordered(concurrency)
            .collect::<Vec<_>>()
            .await;
        had_errors |= outcomes.into_iter().any(|failed| failed);
        let final_status = if self
            .get_run(&run_id)
            .await
            .ok()
            .flatten()
            .is_some_and(|r| matches!(r.status.as_str(), "cancelled" | "cancelling"))
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

    async fn run_accepts_work(&self, run_id: &str) -> bool {
        self.get_run(run_id)
            .await
            .ok()
            .flatten()
            .is_some_and(|run| !matches!(run.status.as_str(), "cancelled" | "cancelling"))
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
        let row = sqlx::query_as::<_, (String,String,String,String,String,String,Option<String>)>("SELECT run_id, project_id, created_at, delivery_directory, status, expedient_ids, error FROM vm_activity_runs WHERE run_id = ?")
            .bind(run_id).fetch_optional(&self.pool).await?;
        Ok(row.map(
            |(run_id, project_id, created_at, delivery_directory, status, ids, error)| RunRecord {
                run_id,
                project_id,
                created_at,
                delivery_directory,
                status,
                expedient_ids: serde_json::from_str(&ids).unwrap_or_default(),
                error,
            },
        ))
    }

    pub async fn list_runs(&self) -> Result<Vec<RunRecord>, ServiceError> {
        let rows = sqlx::query_as::<_, (String,String,String,String,String,String,Option<String>)>(
            "SELECT run_id, project_id, created_at, delivery_directory, status, expedient_ids, error FROM vm_activity_runs ORDER BY created_at DESC, run_id DESC",
        )
        .fetch_all(&self.pool)
        .await?;
        Ok(rows
            .into_iter()
            .map(
                |(run_id, project_id, created_at, delivery_directory, status, ids, error)| {
                    RunRecord {
                        run_id,
                        project_id,
                        created_at,
                        delivery_directory,
                        status,
                        expedient_ids: serde_json::from_str(&ids).unwrap_or_default(),
                        error,
                    }
                },
            )
            .collect())
    }

    pub async fn delete_run(&self, run_id: &str) -> Result<bool, ServiceError> {
        let mut transaction = self.pool.begin().await?;
        let status =
            sqlx::query_scalar::<_, String>("SELECT status FROM vm_activity_runs WHERE run_id=?")
                .bind(run_id)
                .fetch_optional(&mut *transaction)
                .await?;
        let Some(status) = status else {
            transaction.rollback().await?;
            return Ok(false);
        };
        if matches!(status.as_str(), "queued" | "running" | "cancelling") {
            transaction.rollback().await?;
            return Err(ServiceError::Conflict(
                "cannot delete a run while it is queued, running, or cancelling".into(),
            ));
        }
        let delivery_ids = sqlx::query_scalar::<_, String>(
            "SELECT delivery_id FROM vm_activity_expedients WHERE run_id=?",
        )
        .bind(run_id)
        .fetch_all(&mut *transaction)
        .await?;
        sqlx::query("DELETE FROM vm_activity_expedients WHERE run_id=?")
            .bind(run_id)
            .execute(&mut *transaction)
            .await?;
        sqlx::query("DELETE FROM vm_activity_runs WHERE run_id=?")
            .bind(run_id)
            .execute(&mut *transaction)
            .await?;
        for delivery_id in delivery_ids {
            sqlx::query("DELETE FROM vm_activity_deliveries WHERE delivery_id=? AND NOT EXISTS (SELECT 1 FROM vm_activity_expedients WHERE delivery_id=?)")
                .bind(&delivery_id)
                .bind(&delivery_id)
                .execute(&mut *transaction)
                .await?;
        }
        transaction.commit().await?;
        Ok(true)
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
            run.status = "cancelling".into();
            sqlx::query("UPDATE vm_activity_runs SET status = ? WHERE run_id = ?")
                .bind(&run.status)
                .bind(run_id)
                .execute(&self.pool)
                .await?;
        }
        Ok(Some(run))
    }

    pub async fn clean(&self) -> Result<crate::maintenance::MaintenanceReport, ServiceError> {
        let _guard = self.create_gate.lock().await;
        self.ensure_no_active_runs().await?;
        crate::maintenance::clean_orphans(
            &self.pool,
            &self.artifact_root,
            &self.runtime.storage_root,
            &self.runtime,
        )
        .await
        .map_err(ServiceError::Internal)
    }

    pub async fn prune(&self) -> Result<crate::maintenance::MaintenanceReport, ServiceError> {
        let _guard = self.create_gate.lock().await;
        self.ensure_no_active_runs().await?;
        crate::maintenance::prune_runs(
            &self.pool,
            &self.artifact_root,
            &self.runtime.storage_root,
            &self.runtime,
        )
        .await
        .map_err(ServiceError::Internal)
    }

    pub async fn reset(&self) -> Result<crate::maintenance::MaintenanceReport, ServiceError> {
        let _guard = self.create_gate.lock().await;
        self.ensure_no_active_runs().await?;
        crate::maintenance::reset_data(
            &self.pool,
            &self.artifact_root,
            &self.runtime.storage_root,
            &self.runtime,
        )
        .await
        .map_err(ServiceError::Internal)
    }

    async fn ensure_no_active_runs(&self) -> Result<(), ServiceError> {
        let active = sqlx::query_scalar::<_, i64>(
            "SELECT COUNT(*) FROM vm_activity_runs WHERE status IN ('queued','running','cancelling')",
        )
        .fetch_one(&self.pool)
        .await?;
        if active > 0 {
            return Err(ServiceError::Conflict(
                "maintenance is unavailable while a run is queued, running, or cancelling".into(),
            ));
        }
        Ok(())
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
        let report_dir = Path::new(&path).parent().unwrap_or(Path::new("."));
        let result = if report_dir.join("cleanup-manifest.json").is_file() {
            vm_activity::cleanup_from_manifest(&self.runtime, report_dir, &delivery_id).await
        } else {
            serde_json::json!({
                "status": "passed",
                "vm_unregistered": true,
                "run_storage_removed": true,
                "details": ["no VirtualBox import manifest exists; there are no VM resources to clean"]
            })
        };
        report["cleanup"] = result;
        let cleanup_status = report["cleanup"]["status"]
            .as_str()
            .unwrap_or("failed")
            .to_owned();
        let cleanup_observation = report["cleanup"].clone();
        let cleanup_stdout =
            serde_json::to_string_pretty(&cleanup_observation).unwrap_or_else(|_| "{}".to_owned());
        let cleanup_stderr = cleanup_observation["details"]
            .as_array()
            .map(|items| {
                items
                    .iter()
                    .filter_map(serde_json::Value::as_str)
                    .collect::<Vec<_>>()
                    .join("\n")
            })
            .unwrap_or_default();
        let mut cleanup_record_dir = None;
        if let Some(results) = report["test_results"].as_array_mut() {
            if let Some(cleanup) = results
                .iter_mut()
                .find(|r| r["test_reference"] == "VirtualBox/destroy_vm")
            {
                let now = chrono::Utc::now().to_rfc3339();
                if cleanup["start_time"].is_null() {
                    cleanup["start_time"] = serde_json::json!(now);
                }
                cleanup["end_time"] = serde_json::json!(now);
                cleanup["status"] = serde_json::json!(cleanup_status);
                cleanup["exit_code"] =
                    serde_json::json!(if cleanup_status == "passed" { 0 } else { 1 });
                cleanup["stdout"] = serde_json::json!(cleanup_stdout);
                cleanup["stderr"] = serde_json::json!(cleanup_stderr);
                cleanup["observation"] = cleanup_observation.clone();
                cleanup["evidence"] = serde_json::json!(["cleanup-manifest.json"]);
                cleanup["diagnostics"] = if cleanup_status == "passed" {
                    serde_json::Value::Null
                } else {
                    serde_json::json!(cleanup_observation["details"].to_string())
                };
                cleanup_record_dir = cleanup["record_dir"].as_str().map(str::to_owned);
            }
        }
        if let Some(record_dir) = cleanup_record_dir {
            let record_dir = Path::new(&path)
                .parent()
                .unwrap_or(Path::new("."))
                .join(record_dir);
            crate::test_result_store::persist_output(&record_dir, &cleanup_stdout, &cleanup_stderr)
                .map_err(|e| ServiceError::Internal(e.to_string()))?;
        }
        report["finished_at"] = serde_json::json!(chrono::Utc::now().to_rfc3339());
        let no_failed_tests = report["test_results"].as_array().is_some_and(|results| {
            results
                .iter()
                .all(|r| matches!(r["status"].as_str(), Some("passed" | "skipped")))
        }) && report["cleanup"]["status"] == "passed";
        report["overall_status"] = serde_json::json!(if no_failed_tests {
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

fn project_file_component(path: &Path) -> String {
    let filename = path
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default();
    let lower = filename.to_ascii_lowercase();
    let stem = [
        ".thot.tar.gz",
        ".thot.tgz",
        ".thot.gz",
        ".thot.zip",
        ".tar.gz",
        ".tgz",
        ".zip",
    ]
    .iter()
    .find_map(|suffix| {
        lower
            .ends_with(suffix)
            .then(|| filename[..filename.len() - suffix.len()].to_owned())
    })
    .unwrap_or_else(|| {
        Path::new(&filename)
            .file_stem()
            .map(|stem| stem.to_string_lossy().into_owned())
            .unwrap_or(filename)
    });
    safe_id_component(&stem)
}

fn safe_id_component(value: &str) -> String {
    let component: String = value
        .chars()
        .take(100)
        .map(|character| {
            if character.is_ascii_alphanumeric() || matches!(character, '-' | '_') {
                character
            } else {
                '_'
            }
        })
        .collect();
    let component = component.trim_matches('_');
    if component.is_empty() {
        "project".into()
    } else {
        component.to_owned()
    }
}

fn expedient_file_component(value: &str) -> String {
    let component: String = value
        .chars()
        .take(100)
        .map(|character| {
            if character.is_ascii_alphanumeric() || matches!(character, '-' | '_' | '.') {
                character
            } else {
                '_'
            }
        })
        .collect();
    let component = component.trim_matches(|character| character == '_' || character == '.');
    if component.is_empty() {
        "delivery".into()
    } else {
        component.to_owned()
    }
}

fn resolve_directory(path: &Path, relative_to: Option<&Path>) -> Result<PathBuf, String> {
    let requested = if path.is_absolute() {
        path.to_path_buf()
    } else if let Some(base) = relative_to {
        base.join(path)
    } else {
        std::env::current_dir()
            .map_err(|e| format!("cannot resolve current directory: {e}"))?
            .join(path)
    };
    let absolute = if requested.is_absolute() {
        requested.clone()
    } else {
        std::env::current_dir()
            .map_err(|e| format!("cannot resolve current directory: {e}"))?
            .join(&requested)
    };
    let mut current = PathBuf::new();
    for component in absolute.components() {
        current.push(component);
        if fs::symlink_metadata(&current).is_ok_and(|metadata| metadata.file_type().is_symlink()) {
            return Err(format!(
                "delivery directory must not traverse a symlink: {}",
                current.display()
            ));
        }
    }
    let selected = requested.canonicalize().map_err(|e| {
        format!(
            "delivery directory {} is unavailable: {e}",
            requested.display()
        )
    })?;
    if !selected.is_dir() {
        return Err(format!(
            "delivery path {} must resolve to a directory",
            selected.display()
        ));
    }
    Ok(selected)
}

#[cfg(test)]
mod path_tests {
    use super::*;
    use std::time::Duration;

    fn test_runtime() -> VmActivityRuntime {
        VmActivityRuntime {
            vbox_manage: PathBuf::from("/usr/bin/VBoxManage"),
            expected_vbox_version: String::new(),
            vbox_user_home: PathBuf::from("/tmp/thot-vbox-profile"),
            storage_root: PathBuf::from("/tmp/thot-vms"),
            artifact_root: PathBuf::from("/tmp/thot-artifacts"),
            max_ova_bytes: 1024,
            free_space_reserve: 0,
            output_limit: 1024,
            import_timeout: Duration::from_secs(1),
            startup_timeout: Duration::from_secs(1),
            ssh_timeout: Duration::from_secs(1),
            shutdown_grace: Duration::from_secs(1),
            ssh_secret_file: PathBuf::from("/tmp/thot-ssh-secret"),
            firewall_confirmed: true,
        }
    }

    #[test]
    fn delivery_folder_can_be_absolute_or_relative_outside_project_directory() {
        let root = std::env::temp_dir().join(format!("thot-paths-{}", uuid::Uuid::new_v4()));
        let project_dir = root.join("projects").join("linux");
        let deliveries = root.join("external-deliveries");
        std::fs::create_dir_all(&project_dir).unwrap();
        std::fs::create_dir_all(&deliveries).unwrap();

        let relative =
            resolve_directory(Path::new("../../external-deliveries"), Some(&project_dir)).unwrap();
        let absolute = resolve_directory(&deliveries, Some(&project_dir)).unwrap();
        assert_eq!(relative, deliveries.canonicalize().unwrap());
        assert_eq!(absolute, relative);

        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn run_id_uses_project_archive_stem_and_safe_characters() {
        assert_eq!(
            project_file_component(Path::new("linux_competition.thot.tgz")),
            "linux_competition"
        );
        assert_eq!(
            project_file_component(Path::new("My class.thot.zip")),
            "My_class"
        );
    }

    #[test]
    fn expedient_id_component_preserves_filename_extension_safely() {
        assert_eq!(
            expedient_file_component("Alpine Test.Dani.ova"),
            "Alpine_Test.Dani.ova"
        );
    }

    #[tokio::test]
    async fn listing_and_deleting_runs_removes_only_unreferenced_delivery_rows() {
        let root = std::env::temp_dir().join(format!("thot-runs-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        let database_path = root.join("thot.sqlite3");
        std::fs::File::create(&database_path).unwrap();
        let database_url = format!("sqlite:{}", database_path.display());
        let service =
            DeliveryRunService::connect(&database_url, root.join("artifacts"), test_runtime())
                .await
                .unwrap();
        sqlx::query("INSERT INTO vm_activity_runs(run_id,project_id,delivery_directory,status,expedient_ids) VALUES('linux-20261026_154512','project','/delivery','completed','[\"student.ova-20261026_154512\",\"student2.ova-20261026_154512\"]')")
            .execute(&service.pool)
            .await
            .unwrap();
        sqlx::query("INSERT INTO vm_activity_runs(run_id,project_id,delivery_directory,status,expedient_ids) VALUES('other-20261026_154513','project','/delivery','completed','[\"student.ova-20261026_154513\"]')")
            .execute(&service.pool)
            .await
            .unwrap();
        for delivery_id in ["shared-delivery", "orphan-delivery"] {
            sqlx::query("INSERT INTO vm_activity_deliveries(delivery_id,collection_ref,source_ref,content_sha256,original_filename,byte_size) VALUES(?, '/delivery', ?, 'hash', ?, 1)")
                .bind(delivery_id)
                .bind(format!("/delivery/{delivery_id}.ova"))
                .bind(format!("{delivery_id}.ova"))
                .execute(&service.pool)
                .await
                .unwrap();
        }
        sqlx::query("INSERT INTO vm_activity_expedients(expedient_id,run_id,delivery_id,report_path,status) VALUES('student.ova-20261026_154512','linux-20261026_154512','shared-delivery','/reports/one.json','completed')")
            .execute(&service.pool)
            .await
            .unwrap();
        sqlx::query("INSERT INTO vm_activity_expedients(expedient_id,run_id,delivery_id,report_path,status) VALUES('student2.ova-20261026_154512','linux-20261026_154512','orphan-delivery','/reports/two.json','completed')")
            .execute(&service.pool)
            .await
            .unwrap();
        sqlx::query("INSERT INTO vm_activity_expedients(expedient_id,run_id,delivery_id,report_path,status) VALUES('student.ova-20261026_154513','other-20261026_154513','shared-delivery','/reports/other.json','completed')")
            .execute(&service.pool)
            .await
            .unwrap();

        let listed = service.list_runs().await.unwrap();
        assert_eq!(listed.len(), 2);
        assert_eq!(listed[0].run_id, "other-20261026_154513");
        assert_eq!(listed[0].expedient_ids, vec!["student.ova-20261026_154513"]);
        assert_eq!(listed[1].expedient_ids.len(), 2);
        assert!(service.delete_run("linux-20261026_154512").await.unwrap());
        assert!(
            service
                .get_run("linux-20261026_154512")
                .await
                .unwrap()
                .is_none()
        );
        assert_eq!(service.list_runs().await.unwrap().len(), 1);
        let remaining_deliveries: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM vm_activity_deliveries")
                .fetch_one(&service.pool)
                .await
                .unwrap();
        assert_eq!(remaining_deliveries, 1);
        assert!(!service.delete_run("missing-run").await.unwrap());

        sqlx::query("INSERT INTO vm_activity_runs(run_id,project_id,delivery_directory,status) VALUES('active-run','project','/delivery','running')")
            .execute(&service.pool)
            .await
            .unwrap();
        assert!(matches!(
            service.delete_run("active-run").await,
            Err(ServiceError::Conflict(_))
        ));
        drop(service);
        std::fs::remove_dir_all(root).unwrap();
    }
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
