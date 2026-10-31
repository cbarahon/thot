//! Purpose: Coordinates delivery runs, database state, delivery discovery, and expedient cleanup.
use futures_util::stream::{self, StreamExt};
use serde::{Deserialize, Serialize};
use sqlx::{Row, SqlitePool, sqlite::SqlitePoolOptions};
use std::{
    collections::{BTreeMap, HashMap},
    fs,
    path::{Path, PathBuf},
    sync::Arc,
};
use tokio::sync::Mutex;

use crate::{
    delivery_executor, project_context,
    students::import_project_students,
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
    pub expedients: Vec<RunExpedientSummary>,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RunExpedientSummary {
    pub expedient_id: String,
    pub student_id: Option<i64>,
    pub student_name: Option<String>,
    pub student_email: Option<String>,
    pub delivery_id: String,
    pub delivery_filename: String,
    /// Lifecycle state shown to users: pending, running, or finished.
    pub status: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct RunChecksResponse {
    pub run_id: String,
    pub expedients_updated: usize,
    pub expedients: Vec<ExpedientChecksResult>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ExpedientChecksResult {
    pub expedient_id: String,
    pub student_id: Option<i64>,
    pub checks: Vec<serde_json::Value>,
}

#[derive(Debug, Clone)]
struct StudentExpedientWork {
    expedient_id: String,
    student_id: i64,
    student_name: String,
    student_surname: Option<String>,
    student_email: String,
    student_external_id: String,
    delivery_id: String,
    delivery_filename: String,
    report_path: PathBuf,
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
        sqlx::query("CREATE TABLE IF NOT EXISTS vm_activity_expedients (expedient_id TEXT PRIMARY KEY, run_id TEXT NOT NULL, delivery_id TEXT NOT NULL, student_id INTEGER, report_path TEXT NOT NULL, status TEXT NOT NULL, FOREIGN KEY(run_id) REFERENCES vm_activity_runs(run_id), FOREIGN KEY(delivery_id) REFERENCES vm_activity_deliveries(delivery_id))")
            .execute(&pool).await?;
        let columns = sqlx::query("PRAGMA table_info(vm_activity_expedients)")
            .fetch_all(&pool)
            .await?;
        if !columns
            .iter()
            .any(|column| column.try_get::<String, _>("name").ok().as_deref() == Some("student_id"))
        {
            sqlx::query("ALTER TABLE vm_activity_expedients ADD COLUMN student_id INTEGER")
                .execute(&pool)
                .await?;
        }
        crate::project_context::ensure_schema(&pool).await?;
        sqlx::query("CREATE TABLE IF NOT EXISTS student_assignments (student_id INTEGER PRIMARY KEY, delivery_id TEXT NOT NULL, match_score REAL, FOREIGN KEY(student_id) REFERENCES students(id) ON DELETE CASCADE, FOREIGN KEY(delivery_id) REFERENCES vm_activity_deliveries(delivery_id) ON DELETE CASCADE)")
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
        project_context::activate(
            &self.pool,
            &snapshot.project_id,
            &snapshot.configuration.activity_name,
            &snapshot.archive_path,
            Some(&snapshot.sha256),
        )
        .await
        .map_err(|error| ServiceError::Internal(error.to_string()))?;
        let students_csv = snapshot.extracted_root.join("students.csv");
        match std::fs::symlink_metadata(&students_csv) {
            Ok(metadata) if metadata.file_type().is_symlink() || !metadata.is_file() => {
                return Err(ServiceError::Invalid(
                    "project students.csv must be a regular non-symlink file".into(),
                ));
            }
            Ok(_) => {
                let contents = std::fs::read_to_string(&students_csv).map_err(|error| {
                    ServiceError::Invalid(format!("cannot read project students.csv: {error}"))
                })?;
                let (students, malformed) = crate::student_csv::parse_students_csv(&contents)
                    .map_err(ServiceError::Invalid)?;
                let (imported, discarded, rejected) =
                    import_project_students(&self.pool, &snapshot.project_id, students).await?;
                tracing::info!(
                    project_id = %snapshot.project_id,
                    imported,
                    discarded,
                    malformed_rows = malformed.len(),
                    "imported students.csv from project"
                );
                for (row, reason) in malformed {
                    tracing::warn!(project_id = %snapshot.project_id, row, %reason, "skipped malformed project student CSV row");
                }
                for rejected in rejected {
                    tracing::warn!(project_id = %snapshot.project_id, row = ?rejected.row, %rejected.reason, "rejected project student record");
                }
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => {
                return Err(ServiceError::Invalid(format!(
                    "cannot inspect project students.csv: {error}"
                )));
            }
        }
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
        let mut inputs = vm_activity::discover_deliveries(&selected, self.runtime.max_ova_bytes)
            .map_err(ServiceError::Invalid)?;
        if inputs.is_empty() {
            return Err(ServiceError::Invalid(
                "the project delivery folder contains no OVA files to assign".into(),
            ));
        }
        let mut inventory = Vec::with_capacity(inputs.len());
        for input in &mut inputs {
            let delivery_id = register_delivery(&self.pool, input).await?;
            input.delivery_id = delivery_id.clone();
            inventory.push((delivery_id, input.filename.clone()));
        }
        crate::assignments::replace_project_deliveries(
            &self.pool,
            &snapshot.project_id,
            &inventory,
        )
        .await?;
        if let Some(assignments) =
            crate::assignments::load_project_assignments_file(&snapshot.extracted_root)
                .map_err(ServiceError::Invalid)?
        {
            crate::assignments::replace_project_assignments(
                &self.pool,
                &snapshot.project_id,
                &assignments,
            )
            .await
            .map_err(|error| match error {
                crate::assignments::AssignmentError::Invalid(message) => {
                    ServiceError::Invalid(message)
                }
                crate::assignments::AssignmentError::Storage(error) => {
                    ServiceError::Internal(format!("cannot import project assignments: {error}"))
                }
                crate::assignments::AssignmentError::Project(error) => {
                    ServiceError::Internal(format!("cannot resolve project assignments: {error}"))
                }
            })?;
        }

        let student_ids = sqlx::query_scalar::<_, i64>(
            "SELECT student_id FROM project_students WHERE project_id=? ORDER BY student_id",
        )
        .bind(&snapshot.project_id)
        .fetch_all(&self.pool)
        .await?;
        if student_ids.is_empty() {
            return Err(ServiceError::Invalid(
                "the project has no students; import a roster before assigning deliveries and creating a run".into(),
            ));
        }
        let assignments = sqlx::query_as::<_, (i64, String, Option<String>, String, String, String, String)>(
            "SELECT ps.student_id, ps.name, ps.surname, ps.email, ps.external_id, pa.delivery_id, d.original_filename FROM project_assignments pa JOIN project_students ps ON ps.project_id=pa.project_id AND ps.student_id=pa.student_id JOIN vm_activity_deliveries d ON d.delivery_id=pa.delivery_id WHERE pa.project_id=? ORDER BY ps.student_id",
        )
        .bind(&snapshot.project_id)
        .fetch_all(&self.pool)
        .await?;
        let mut student_work = Vec::with_capacity(assignments.len());
        for (
            student_id,
            student_name,
            student_surname,
            student_email,
            student_external_id,
            delivery_id,
            delivery_filename,
        ) in assignments
        {
            let base = format!(
                "{}-{timestamp}-student-{student_id}",
                expedient_file_component(&delivery_filename)
            );
            let mut expedient_id = base.clone();
            let mut collision = 1_u32;
            while sqlx::query_scalar::<_, i64>(
                "SELECT COUNT(*) FROM vm_activity_expedients WHERE expedient_id=?",
            )
            .bind(&expedient_id)
            .fetch_one(&self.pool)
            .await?
                > 0
            {
                expedient_id = format!("{base}-{collision:02}");
                collision += 1;
            }
            let report_dir = self
                .artifact_root
                .join(&snapshot.project_id)
                .join(&run_id)
                .join(&delivery_id);
            let report_path = report_dir.join(format!("{expedient_id}.json"));
            student_work.push(StudentExpedientWork {
                expedient_id,
                student_id,
                student_name,
                student_surname,
                student_email,
                student_external_id,
                delivery_id,
                delivery_filename,
                report_path,
            });
        }
        let expedient_ids = student_work
            .iter()
            .map(|work| work.expedient_id.clone())
            .collect::<Vec<_>>();
        let exp_data = serde_json::to_string(&expedient_ids).unwrap_or_else(|_| "[]".into());
        let mut transaction = self.pool.begin().await?;
        sqlx::query("INSERT INTO vm_activity_runs(run_id,project_id,delivery_directory,status,expedient_ids,created_at) VALUES(?,?,?,?,?,?)")
            .bind(&run_id).bind(&snapshot.project_id).bind(selected.display().to_string()).bind("queued").bind(exp_data).bind(&created_at).execute(&mut *transaction).await?;
        for work in &student_work {
            sqlx::query("INSERT INTO vm_activity_expedients(expedient_id,run_id,delivery_id,student_id,report_path,status) VALUES(?,?,?,?,?,?)")
                .bind(&work.expedient_id)
                .bind(&run_id)
                .bind(&work.delivery_id)
                .bind(work.student_id)
                .bind(work.report_path.display().to_string())
                .bind("pending")
                .execute(&mut *transaction)
                .await?;
        }
        transaction.commit().await?;
        let expedients = student_work
            .iter()
            .map(|work| RunExpedientSummary {
                expedient_id: work.expedient_id.clone(),
                student_id: Some(work.student_id),
                student_name: Some(work.student_name.clone()),
                student_email: Some(work.student_email.clone()),
                delivery_id: work.delivery_id.clone(),
                delivery_filename: work.delivery_filename.clone(),
                status: "pending".into(),
            })
            .collect();
        let record = RunRecord {
            run_id: run_id.clone(),
            project_id: snapshot.project_id.clone(),
            created_at: created_at.clone(),
            delivery_directory: selected.display().to_string(),
            status: "queued".into(),
            expedient_ids,
            expedients,
            error: None,
        };
        let service = self.clone();
        let background_run_id = record.run_id.clone();
        tokio::spawn(async move {
            service
                .execute_run(
                    background_run_id,
                    snapshot,
                    vbox_version,
                    inputs,
                    student_work,
                )
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
        students: Vec<StudentExpedientWork>,
    ) {
        let _ = sqlx::query(
            "UPDATE vm_activity_runs SET status='running' WHERE run_id=? AND status='queued'",
        )
        .bind(&run_id)
        .execute(&self.pool)
        .await;
        struct DeliveryWork {
            input: DeliveryInput,
            students: Vec<StudentExpedientWork>,
            report_dir: PathBuf,
        }
        let mut inputs_by_id = inputs
            .into_iter()
            .map(|input| (input.delivery_id.clone(), input))
            .collect::<HashMap<_, _>>();
        let mut students_by_delivery = BTreeMap::<String, Vec<StudentExpedientWork>>::new();
        for student in students {
            students_by_delivery
                .entry(student.delivery_id.clone())
                .or_default()
                .push(student);
        }
        let expected_delivery_count = students_by_delivery.len();
        let mut work_items = Vec::with_capacity(expected_delivery_count);
        for (delivery_id, mut assigned_students) in students_by_delivery {
            assigned_students.sort_by_key(|student| student.student_id);
            let Some(input) = inputs_by_id.remove(&delivery_id) else {
                tracing::error!(%delivery_id, "assigned delivery disappeared before execution");
                continue;
            };
            let report_dir = assigned_students[0]
                .report_path
                .parent()
                .unwrap_or(Path::new("."))
                .to_path_buf();
            work_items.push(DeliveryWork {
                input,
                students: assigned_students,
                report_dir,
            });
        }
        let mut had_errors = work_items.len() != expected_delivery_count;
        let concurrency = project.configuration.concurrency.max(1) as usize;
        let outcomes = stream::iter(work_items)
            .map(|mut work| {
                let service = self;
                let run_id = run_id.clone();
                let project = project.clone();
                let vbox_version = vbox_version.clone();
                async move {
                if !service.run_accepts_work(&run_id).await {
                    return false;
                }
                if let Err(error) = sqlx::query("UPDATE vm_activity_expedients SET status='running' WHERE run_id=? AND delivery_id=? AND status='pending'")
                    .bind(&run_id).bind(&work.input.delivery_id).execute(&service.pool).await
                {
                    tracing::error!(%error, delivery_id = %work.input.delivery_id, "could not mark student expedients running");
                    return true;
                }
                if work.input.validation_error.is_none() {
                    match vm_activity::retain_source(&service.runtime, &work.input).await {
                        Ok(source) => work.input.source = source,
                        Err(error) => work.input.validation_error = Some(error),
                    }
                }
                let execution_expedient_id = work.students[0].expedient_id.clone();
                let execution_report_path = work.students[0].report_path.clone();
                let report = delivery_executor::execute_delivery(
                    &service.runtime,
                    &project,
                    &vbox_version,
                    &run_id,
                    &execution_expedient_id,
                    &work.input,
                    &work.report_dir,
                    &execution_report_path,
                ).await;
                let result_status = report["overall_status"].as_str().unwrap_or("completed_with_errors");
                let mut copy_failed = false;
                for student in &work.students {
                    let mut student_report = report.clone();
                    student_report["schema_version"] = serde_json::json!(3);
                    student_report["expedient_id"] = serde_json::json!(student.expedient_id);
                    student_report["student"] = serde_json::json!({
                        "student_id": student.student_id,
                        "name": student.student_name,
                        "surname": student.student_surname,
                        "email": student.student_email,
                        "external_id": student.student_external_id
                    });
                    student_report["delivery_filename"] = serde_json::json!(student.delivery_filename);
                    student_report["shared_delivery_execution"] = serde_json::json!(work.students.len() > 1);
                    student_report["evaluation"] = serde_json::json!({
                        "status": "not_evaluated",
                        "results": []
                    });
                    if let Err(error) = crate::test_result_store::persist_report(&student.report_path, &student_report) {
                        tracing::error!(%error, expedient_id = %student.expedient_id, "could not persist student expedient report");
                        copy_failed = true;
                    }
                    if let Err(error) = sqlx::query("UPDATE vm_activity_expedients SET status='finished', report_path=? WHERE expedient_id=?")
                        .bind(student.report_path.display().to_string())
                        .bind(&student.expedient_id)
                        .execute(&service.pool)
                        .await
                    {
                        tracing::error!(%error, expedient_id = %student.expedient_id, "could not save student expedient status");
                        copy_failed = true;
                    }
                }
                result_status != "completed" || copy_failed
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
        sqlx::query_scalar::<_, String>("SELECT status FROM vm_activity_runs WHERE run_id=?")
            .bind(run_id)
            .fetch_optional(&self.pool)
            .await
            .ok()
            .flatten()
            .is_some_and(|status| !matches!(status.as_str(), "cancelled" | "cancelling"))
    }

    pub async fn get_run(&self, run_id: &str) -> Result<Option<RunRecord>, ServiceError> {
        let row = sqlx::query_as::<_, (String,String,String,String,String,String,Option<String>)>("SELECT run_id, project_id, created_at, delivery_directory, status, expedient_ids, error FROM vm_activity_runs WHERE run_id = ?")
            .bind(run_id).fetch_optional(&self.pool).await?;
        let Some((run_id, project_id, created_at, delivery_directory, status, ids, error)) = row
        else {
            return Ok(None);
        };
        let expedient_ids = serde_json::from_str(&ids).unwrap_or_default();
        let expedients = self.list_run_expedients(&run_id).await?;
        Ok(Some(RunRecord {
            run_id,
            project_id,
            created_at,
            delivery_directory,
            status,
            expedient_ids,
            expedients,
            error,
        }))
    }

    pub async fn list_runs(&self) -> Result<Vec<RunRecord>, ServiceError> {
        let rows = sqlx::query_as::<_, (String,String,String,String,String,String,Option<String>)>(
            "SELECT run_id, project_id, created_at, delivery_directory, status, expedient_ids, error FROM vm_activity_runs ORDER BY created_at DESC, run_id DESC",
        )
        .fetch_all(&self.pool)
        .await?;
        let mut runs = Vec::with_capacity(rows.len());
        for (run_id, project_id, created_at, delivery_directory, status, ids, error) in rows {
            runs.push(RunRecord {
                expedients: self.list_run_expedients(&run_id).await?,
                run_id,
                project_id,
                created_at,
                delivery_directory,
                status,
                expedient_ids: serde_json::from_str(&ids).unwrap_or_default(),
                error,
            });
        }
        Ok(runs)
    }

    /// Re-evaluate configured checks against saved test results for a finished run.
    pub async fn run_checks(
        &self,
        requested_run_id: Option<&str>,
        requested_project_path: Option<&str>,
    ) -> Result<Option<RunChecksResponse>, ServiceError> {
        let _guard = self.create_gate.lock().await;
        let run = if let Some(run_id) = requested_run_id {
            sqlx::query_as::<_, (String, String, String)>(
                "SELECT run_id, status, project_id FROM vm_activity_runs WHERE run_id=?",
            )
            .bind(run_id)
            .fetch_optional(&self.pool)
            .await?
        } else {
            sqlx::query_as::<_, (String, String, String)>(
                "SELECT run_id, status, project_id FROM vm_activity_runs WHERE status NOT IN ('queued','running','cancelling') ORDER BY created_at DESC, run_id DESC LIMIT 1",
            )
            .fetch_optional(&self.pool)
            .await?
        };
        let Some((run_id, status, project_id)) = run else {
            if requested_run_id.is_some() {
                return Ok(None);
            }
            return Err(ServiceError::Invalid(
                "there is no finished run to check; specify a RUN_ID or run tests first".into(),
            ));
        };
        if matches!(status.as_str(), "queued" | "running" | "cancelling") {
            return Err(ServiceError::Conflict(format!(
                "cannot run checks for run {run_id:?} while its tests are still running"
            )));
        }

        let snapshot_configuration_path = self
            .artifact_root
            .join("project-snapshots")
            .join(&run_id)
            .join("project")
            .join("configuration.yaml");
        let current_configuration_path = if let Some(project_path) = requested_project_path {
            let path = Path::new(project_path);
            let matches_run_project = crate::vm_activity::project_identifier_for_path(path)
                .is_ok_and(|candidate_id| candidate_id == project_id);
            if matches_run_project && path.is_dir() {
                Some(path.join("configuration.yaml"))
            } else {
                None
            }
        } else {
            None
        };
        let configuration_path = current_configuration_path
            .as_deref()
            .unwrap_or(&snapshot_configuration_path);
        let metadata = fs::symlink_metadata(&configuration_path).map_err(|error| {
            ServiceError::Internal(format!(
                "project configuration for run {run_id:?} is unavailable at {}: {error}",
                configuration_path.display()
            ))
        })?;
        if metadata.file_type().is_symlink() || !metadata.is_file() {
            return Err(ServiceError::Internal(format!(
                "project configuration for run {run_id:?} is not a regular file: {}",
                configuration_path.display()
            )));
        }
        let configuration_bytes = fs::read(&configuration_path).map_err(|error| {
            ServiceError::Internal(format!(
                "cannot read project configuration for run {run_id:?}: {error}"
            ))
        })?;
        let project =
            crate::project_config::parse_configuration(&configuration_bytes).map_err(|error| {
                ServiceError::Internal(format!("project configuration for run is invalid: {error}"))
            })?;
        if project.checks.is_empty() {
            return Err(ServiceError::Invalid(format!(
                "no checks are configured for run {run_id:?} in the project configuration used at {}",
                configuration_path.display()
            )));
        }

        let expedients = sqlx::query_as::<_, (String, Option<i64>, String)>(
            "SELECT expedient_id, student_id, report_path FROM vm_activity_expedients WHERE run_id=? ORDER BY student_id, expedient_id",
        )
        .bind(&run_id)
        .fetch_all(&self.pool)
        .await?;
        let mut reports = Vec::with_capacity(expedients.len());
        for (expedient_id, student_id, report_path) in expedients {
            let bytes = fs::read(&report_path).map_err(|error| {
                ServiceError::Internal(format!(
                    "cannot read expedient {expedient_id:?} report at {report_path}: {error}"
                ))
            })?;
            let mut report: serde_json::Value =
                serde_json::from_slice(&bytes).map_err(|error| {
                    ServiceError::Internal(format!(
                        "expedient {expedient_id:?} report is invalid JSON: {error}"
                    ))
                })?;
            let check_rows =
                crate::checks::evaluate_checks(&project.checks, &project.tests, &report);
            report["checks"] = serde_json::json!(check_rows);
            reports.push((
                expedient_id,
                student_id,
                PathBuf::from(report_path),
                report,
                check_rows,
            ));
        }

        let mut result_rows = Vec::with_capacity(reports.len());
        for (expedient_id, student_id, report_path, report, checks) in reports {
            crate::test_result_store::persist_report(&report_path, &report).map_err(|error| {
                ServiceError::Internal(format!(
                    "cannot save checks to expedient {expedient_id:?}: {error}"
                ))
            })?;
            result_rows.push(ExpedientChecksResult {
                expedient_id,
                student_id,
                checks,
            });
        }
        Ok(Some(RunChecksResponse {
            run_id,
            expedients_updated: result_rows.len(),
            expedients: result_rows,
        }))
    }

    async fn list_run_expedients(
        &self,
        run_id: &str,
    ) -> Result<Vec<RunExpedientSummary>, sqlx::Error> {
        let rows = sqlx::query_as::<_, (String, Option<i64>, Option<String>, Option<String>, String, String, String)>(
            "SELECT e.expedient_id, e.student_id, ps.name, ps.email, e.delivery_id, d.original_filename, e.status FROM vm_activity_expedients e JOIN vm_activity_runs run ON run.run_id=e.run_id LEFT JOIN project_students ps ON ps.project_id=run.project_id AND ps.student_id=e.student_id LEFT JOIN vm_activity_deliveries d ON d.delivery_id=e.delivery_id WHERE e.run_id=? ORDER BY ps.student_id, e.expedient_id",
        )
        .bind(run_id)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows
            .into_iter()
            .map(
                |(
                    expedient_id,
                    student_id,
                    student_name,
                    student_email,
                    delivery_id,
                    delivery_filename,
                    status,
                )| {
                    let status = match status.as_str() {
                        "pending" | "queued" => "pending",
                        "running" => "running",
                        _ => "finished",
                    };
                    RunExpedientSummary {
                        expedient_id,
                        student_id,
                        student_name,
                        student_email,
                        delivery_id,
                        delivery_filename,
                        status: status.into(),
                    }
                },
            )
            .collect())
    }

    pub async fn delete_run(&self, run_id: &str) -> Result<bool, ServiceError> {
        let mut transaction = self.pool.begin().await?;
        let run =
            sqlx::query_scalar::<_, String>("SELECT status FROM vm_activity_runs WHERE run_id=?")
                .bind(run_id)
                .fetch_optional(&mut *transaction)
                .await?;
        let Some(status) = run else {
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
            sqlx::query("DELETE FROM student_assignments WHERE delivery_id=? AND NOT EXISTS (SELECT 1 FROM vm_activity_expedients WHERE delivery_id=?)")
                .bind(&delivery_id)
                .bind(&delivery_id)
                .execute(&mut *transaction)
                .await?;
            sqlx::query("DELETE FROM vm_activity_deliveries WHERE delivery_id=? AND NOT EXISTS (SELECT 1 FROM vm_activity_expedients WHERE delivery_id=?) AND NOT EXISTS (SELECT 1 FROM project_deliveries WHERE delivery_id=?)")
                .bind(&delivery_id)
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
        let row = sqlx::query_as::<_, (String, String, String)>(
            "SELECT report_path,delivery_id,run_id FROM vm_activity_expedients WHERE expedient_id=?",
        )
        .bind(expedient_id)
        .fetch_optional(&self.pool)
        .await?;
        let Some((path, delivery_id, run_id)) = row else {
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
        let sibling_paths = sqlx::query_scalar::<_, String>(
            "SELECT report_path FROM vm_activity_expedients WHERE run_id=? AND delivery_id=?",
        )
        .bind(&run_id)
        .bind(&delivery_id)
        .fetch_all(&self.pool)
        .await?;
        for sibling_path in sibling_paths {
            if sibling_path == path {
                continue;
            }
            let Ok(bytes) = tokio::fs::read(&sibling_path).await else {
                continue;
            };
            let Ok(mut sibling) = serde_json::from_slice::<serde_json::Value>(&bytes) else {
                continue;
            };
            sibling["cleanup"] = report["cleanup"].clone();
            sibling["finished_at"] = report["finished_at"].clone();
            sibling["overall_status"] = report["overall_status"].clone();
            if let (Some(source_results), Some(target_results)) = (
                report["test_results"].as_array(),
                sibling["test_results"].as_array_mut(),
            ) {
                if let Some(cleanup) = source_results
                    .iter()
                    .find(|result| result["test_reference"] == "VirtualBox/destroy_vm")
                {
                    if let Some(target) = target_results
                        .iter_mut()
                        .find(|result| result["test_reference"] == "VirtualBox/destroy_vm")
                    {
                        *target = cleanup.clone();
                    }
                }
            }
            crate::test_result_store::persist_report(Path::new(&sibling_path), &sibling)
                .map_err(|error| ServiceError::Internal(error.to_string()))?;
        }
        sqlx::query(
            "UPDATE vm_activity_expedients SET status='finished' WHERE run_id=? AND delivery_id=?",
        )
        .bind(&run_id)
        .bind(&delivery_id)
        .execute(&self.pool)
        .await?;
        if report["cleanup"]["status"] == "passed" {
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

pub(crate) fn resolve_directory(
    path: &Path,
    relative_to: Option<&Path>,
) -> Result<PathBuf, String> {
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

pub(crate) async fn register_delivery(
    pool: &SqlitePool,
    input: &DeliveryInput,
) -> Result<String, sqlx::Error> {
    let collection_ref = input
        .original_source
        .parent()
        .unwrap_or(Path::new(""))
        .display()
        .to_string();
    let source_ref = input.original_source.display().to_string();
    let content_sha256 = input.sha256.clone().unwrap_or_default();
    sqlx::query("INSERT OR IGNORE INTO vm_activity_deliveries(delivery_id,collection_ref,source_ref,content_sha256,original_filename,byte_size) VALUES(?,?,?,?,?,?)")
        .bind(&input.delivery_id).bind(&collection_ref).bind(&source_ref).bind(&content_sha256).bind(&input.filename).bind(input.size as i64).execute(pool).await?;
    sqlx::query_scalar("SELECT delivery_id FROM vm_activity_deliveries WHERE collection_ref=? AND source_ref=? AND content_sha256=?")
        .bind(collection_ref).bind(source_ref).bind(content_sha256).fetch_one(pool).await
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
