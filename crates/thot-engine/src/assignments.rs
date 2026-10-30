//! Purpose: Guesses, stores, and reports student-to-delivery assignments.
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sqlx::{
    SqlitePool,
    sqlite::{SqliteConnectOptions, SqlitePoolOptions},
};
use std::{
    collections::{HashMap, HashSet},
    fs,
    path::Path,
    str::FromStr,
};
use thiserror::Error;

use crate::{
    delivery_run_service::{self, register_delivery},
    project_context::{self, ProjectContext, ProjectContextError},
    students::import_project_students,
    vm_activity,
};

const MINIMUM_MATCH_SCORE: f64 = 0.56;
const AMBIGUOUS_MATCH_MARGIN: f64 = 0.04;

#[derive(Debug, Error)]
pub enum AssignmentError {
    #[error("invalid assignment data: {0}")]
    Invalid(String),
    #[error("assignment storage error: {0}")]
    Storage(#[from] sqlx::Error),
    #[error(transparent)]
    Project(#[from] ProjectContextError),
}

#[derive(Debug, Deserialize)]
pub struct ImportAssignmentsRequest {
    #[serde(default)]
    pub project_path: Option<String>,
    #[serde(default)]
    pub assignments: Vec<Value>,
}

#[derive(Debug, Deserialize)]
struct AssignmentFile {
    assignments: Vec<Value>,
}

#[derive(Debug, Clone, Serialize)]
pub struct StudentAssignment {
    pub student_id: i64,
    pub student_name: String,
    pub student_surname: Option<String>,
    pub student_email: String,
    pub student_external_id: Option<String>,
    pub delivery_id: Option<String>,
    pub delivery_filename: Option<String>,
    pub assignment_count: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub match_score: Option<f64>,
}

#[derive(Debug, Clone, Serialize)]
pub struct DeliveryAssignmentSummary {
    pub delivery_id: String,
    pub filename: String,
    pub assignment_count: usize,
}

#[derive(Debug, Clone, Serialize)]
pub struct AssignmentsResponse {
    pub project_id: String,
    pub project_name: String,
    pub project_path: String,
    pub assignments: Vec<StudentAssignment>,
    pub unassigned_deliverables: Vec<DeliveryAssignmentSummary>,
}

#[derive(Debug, Clone, Serialize)]
pub struct AssignmentRecordError {
    /// One-based index in the submitted assignments array. Missing project rows have no index.
    pub record_index: Option<usize>,
    pub student_external_id: Option<String>,
    pub error: String,
    pub correction: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct AssignmentImportResponse {
    #[serde(flatten)]
    pub assignments: AssignmentsResponse,
    pub imported_records: usize,
    pub errors: Vec<AssignmentRecordError>,
}

#[derive(Clone)]
pub struct AssignmentService {
    pool: SqlitePool,
}

impl AssignmentService {
    pub async fn connect(database_url: &str) -> Result<Self, AssignmentError> {
        if let Some(path) = database_url.strip_prefix("sqlite:") {
            let path = path.split('?').next().unwrap_or(path);
            if path != ":memory:" {
                if let Some(parent) = Path::new(path).parent() {
                    if !parent.as_os_str().is_empty() {
                        std::fs::create_dir_all(parent).map_err(|error| {
                            AssignmentError::Invalid(format!(
                                "cannot create assignment database directory: {error}"
                            ))
                        })?;
                    }
                }
            }
        }
        let options = SqliteConnectOptions::from_str(database_url)
            .map_err(|error| AssignmentError::Invalid(error.to_string()))?
            .create_if_missing(true)
            .foreign_keys(true);
        let pool = SqlitePoolOptions::new()
            .max_connections(5)
            .connect_with(options)
            .await?;
        project_context::ensure_schema(&pool).await?;
        Ok(Self { pool })
    }

    /// Recomputes and stores the best non-ambiguous filename match for each student.
    pub async fn guess(
        &self,
        requested_project_path: Option<&str>,
    ) -> Result<AssignmentsResponse, AssignmentError> {
        let project = project_context::resolve(&self.pool, requested_project_path).await?;
        self.refresh_project_inventory(&project).await?;
        let students = sqlx::query_as::<_, StudentMatchRow>(
            "SELECT s.id, ps.name, ps.surname, ps.email, ps.external_id FROM students s JOIN project_students ps ON ps.student_id=s.id WHERE ps.project_id=? ORDER BY s.id",
        )
        .bind(&project.project_id)
        .fetch_all(&self.pool)
        .await?;
        let deliveries = sqlx::query_as::<_, DeliveryRow>(
            "SELECT d.delivery_id, d.original_filename FROM project_deliveries pd JOIN vm_activity_deliveries d ON d.delivery_id=pd.delivery_id WHERE pd.project_id=? ORDER BY d.original_filename, d.delivery_id",
        )
        .bind(&project.project_id)
        .fetch_all(&self.pool)
        .await?;

        let mut suggestions = Vec::new();
        for student in students {
            let mut scored = deliveries
                .iter()
                .map(|delivery| {
                    (
                        delivery,
                        score_student_filename(&student, &delivery.original_filename),
                    )
                })
                .filter(|(_, score)| *score >= MINIMUM_MATCH_SCORE)
                .collect::<Vec<_>>();
            scored.sort_by(|left, right| {
                right
                    .1
                    .total_cmp(&left.1)
                    .then_with(|| left.0.delivery_id.cmp(&right.0.delivery_id))
            });
            let best = scored.first().copied();
            let ambiguous = match (best, scored.get(1)) {
                (Some((_, best_score)), Some((_, second_score))) => {
                    best_score - second_score < AMBIGUOUS_MATCH_MARGIN
                }
                _ => false,
            };
            if let Some((delivery, score)) = best.filter(|_| !ambiguous) {
                suggestions.push((student.id, delivery.delivery_id.clone(), score));
            }
        }

        let mut transaction = self.pool.begin().await?;
        sqlx::query("DELETE FROM project_assignments WHERE project_id=?")
            .bind(&project.project_id)
            .execute(&mut *transaction)
            .await?;
        for (student_id, delivery_id, score) in suggestions {
            sqlx::query(
                "INSERT INTO project_assignments(project_id, student_id, delivery_id, match_score) VALUES(?, ?, ?, ?)",
            )
            .bind(&project.project_id)
            .bind(student_id)
            .bind(delivery_id)
            .bind(score)
            .execute(&mut *transaction)
            .await?;
        }
        transaction.commit().await?;
        self.get(None).await
    }

    /// Returns one row per student and the deliveries not assigned to anyone.
    pub async fn get(
        &self,
        requested_project_path: Option<&str>,
    ) -> Result<AssignmentsResponse, AssignmentError> {
        let project = project_context::resolve(&self.pool, requested_project_path).await?;
        self.refresh_project_inventory(&project).await?;
        let rows = sqlx::query_as::<_, AssignmentViewRow>(
            r#"SELECT s.id, ps.name, ps.surname, ps.email, ps.external_id,
                      a.delivery_id, d.original_filename,
                      COALESCE((SELECT COUNT(*) FROM project_assignments counts
                                WHERE counts.project_id = ps.project_id
                                  AND counts.delivery_id = a.delivery_id), 0) AS assignment_count,
                      a.match_score
               FROM students s
               JOIN project_students ps ON ps.student_id = s.id
               LEFT JOIN project_assignments a ON a.student_id = s.id AND a.project_id=ps.project_id
                    AND EXISTS (
                        SELECT 1 FROM project_deliveries pd
                        WHERE pd.delivery_id=a.delivery_id AND pd.project_id=ps.project_id
                    )
               LEFT JOIN vm_activity_deliveries d ON d.delivery_id = a.delivery_id
               WHERE ps.project_id=?
               ORDER BY s.id"#,
        )
        .bind(&project.project_id)
        .fetch_all(&self.pool)
        .await?;
        let assignments = rows
            .into_iter()
            .map(|row| StudentAssignment {
                student_id: row.id,
                student_name: row.name,
                student_surname: row.surname,
                student_email: row.email,
                student_external_id: row.external_id,
                delivery_id: row.delivery_id,
                delivery_filename: row.original_filename,
                assignment_count: row.assignment_count.max(0) as usize,
                match_score: row.match_score,
            })
            .collect();
        let unassigned_deliverables = sqlx::query_as::<_, UnassignedDeliveryRow>(
            r#"SELECT d.delivery_id, d.original_filename,
                      (SELECT COUNT(*) FROM project_assignments a
                       WHERE a.project_id=? AND a.delivery_id = d.delivery_id) AS assignment_count
               FROM project_deliveries pd
               JOIN vm_activity_deliveries d ON d.delivery_id=pd.delivery_id
               WHERE pd.project_id=?
               AND NOT EXISTS (
                   SELECT 1 FROM project_assignments a WHERE a.project_id=? AND a.delivery_id = d.delivery_id
               )
               GROUP BY d.delivery_id, d.original_filename
               ORDER BY d.original_filename, d.delivery_id"#,
        )
        .bind(&project.project_id)
        .bind(&project.project_id)
        .bind(&project.project_id)
        .fetch_all(&self.pool)
        .await?
        .into_iter()
        .map(|row| DeliveryAssignmentSummary {
            delivery_id: row.delivery_id,
            filename: row.original_filename,
            assignment_count: row.assignment_count.max(0) as usize,
        })
        .collect();
        Ok(AssignmentsResponse {
            project_id: project.project_id,
            project_name: project.project_name,
            project_path: project.project_path,
            assignments,
            unassigned_deliverables,
        })
    }

    /// Replaces this project's saved assignments using external IDs and delivery filenames.
    pub async fn import(
        &self,
        request: ImportAssignmentsRequest,
    ) -> Result<AssignmentImportResponse, AssignmentError> {
        let project = project_context::resolve(&self.pool, request.project_path.as_deref()).await?;
        self.refresh_project_inventory(&project).await?;
        let resolved =
            resolve_project_assignments(&self.pool, &project.project_id, &request.assignments)
                .await?;
        persist_project_assignments(&self.pool, &project.project_id, &resolved.rows).await?;
        let assignments = self.get(None).await?;
        Ok(AssignmentImportResponse {
            assignments,
            imported_records: resolved.imported_records,
            errors: resolved.errors,
        })
    }

    async fn refresh_project_inventory(
        &self,
        project: &ProjectContext,
    ) -> Result<(), AssignmentError> {
        let staging = std::env::temp_dir().join(format!(
            "thot-assignment-index-{}",
            uuid::Uuid::new_v4().simple()
        ));
        let result = self.refresh_project_inventory_at(project, &staging).await;
        let _ = std::fs::remove_dir_all(&staging);
        result
    }

    async fn refresh_project_inventory_at(
        &self,
        project: &ProjectContext,
        staging: &Path,
    ) -> Result<(), AssignmentError> {
        let snapshot = vm_activity::load_project(Path::new(&project.project_path), staging)
            .await
            .map_err(AssignmentError::Invalid)?;
        let students_csv = snapshot.extracted_root.join("students.csv");
        match std::fs::symlink_metadata(&students_csv) {
            Ok(metadata) if metadata.file_type().is_symlink() || !metadata.is_file() => {
                return Err(AssignmentError::Invalid(
                    "project students.csv must be a regular non-symlink file".into(),
                ));
            }
            Ok(_) => {
                let contents = std::fs::read_to_string(&students_csv).map_err(|error| {
                    AssignmentError::Invalid(format!("cannot read project students.csv: {error}"))
                })?;
                let (students, malformed) = crate::student_csv::parse_students_csv(&contents)
                    .map_err(AssignmentError::Invalid)?;
                let (_, _, rejected) =
                    import_project_students(&self.pool, &project.project_id, students).await?;
                if !malformed.is_empty() || !rejected.is_empty() {
                    tracing::warn!(
                        project_id = %project.project_id,
                        malformed_rows = malformed.len(),
                        rejected_rows = rejected.len(),
                        "some project students.csv rows were not imported"
                    );
                }
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => {
                return Err(AssignmentError::Invalid(format!(
                    "cannot inspect project students.csv: {error}"
                )));
            }
        }
        let delivery_directory = delivery_run_service::resolve_directory(
            Path::new(&snapshot.configuration.delivery_folder),
            Some(&snapshot.archive_parent),
        )
        .map_err(AssignmentError::Invalid)?;
        let deliveries = vm_activity::discover_deliveries(&delivery_directory, u64::MAX)
            .map_err(AssignmentError::Invalid)?;
        let mut inventory = Vec::with_capacity(deliveries.len());
        for delivery in &deliveries {
            let delivery_id = register_delivery(&self.pool, delivery).await?;
            inventory.push((delivery_id, delivery.filename.clone()));
        }
        replace_project_deliveries(&self.pool, &project.project_id, &inventory).await?;
        Ok(())
    }
}

/// Read the optional project-root assignment file, rejecting ambiguous dual formats.
pub(crate) fn load_project_assignments_file(
    project_root: &Path,
) -> Result<Option<Vec<Value>>, String> {
    let yaml_path = project_root.join("assignments.yaml");
    let json_path = project_root.join("assignments.json");
    let yaml_exists = regular_file_exists(&yaml_path)?;
    let json_exists = regular_file_exists(&json_path)?;
    if yaml_exists && json_exists {
        return Err("project contains both assignments.yaml and assignments.json".into());
    }
    let path = if yaml_exists {
        yaml_path
    } else if json_exists {
        json_path
    } else {
        return Ok(None);
    };
    let bytes = fs::read(&path).map_err(|error| {
        format!(
            "cannot read project assignment file {}: {error}",
            path.display()
        )
    })?;
    let file: AssignmentFile = if path.extension().and_then(|value| value.to_str()) == Some("json")
    {
        serde_json::from_slice(&bytes).map_err(|error| {
            format!(
                "invalid project assignment file {}: {error}",
                path.display()
            )
        })?
    } else {
        serde_yaml::from_slice(&bytes).map_err(|error| {
            format!(
                "invalid project assignment file {}: {error}",
                path.display()
            )
        })?
    };
    Ok(Some(file.assignments))
}

fn regular_file_exists(path: &Path) -> Result<bool, String> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_symlink() || !metadata.is_file() => Err(format!(
            "project assignment file {} must be a regular non-symlink file",
            path.display()
        )),
        Ok(_) => Ok(true),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(format!(
            "cannot inspect project assignment file {}: {error}",
            path.display()
        )),
    }
}

/// Atomically replace project assignments after resolving identities against current project data.
pub(crate) async fn replace_project_assignments(
    pool: &SqlitePool,
    project_id: &str,
    assignments: &[Value],
) -> Result<(), AssignmentError> {
    let resolved = resolve_project_assignments(pool, project_id, assignments).await?;
    if !resolved.errors.is_empty() {
        let details = resolved
            .errors
            .iter()
            .map(|error| {
                let record = error
                    .record_index
                    .map(|index| format!("record {index}: "))
                    .unwrap_or_default();
                format!("{record}{}; correction: {}", error.error, error.correction)
            })
            .collect::<Vec<_>>()
            .join("; ");
        return Err(AssignmentError::Invalid(format!(
            "assignment file has {} error(s): {details}",
            resolved.errors.len()
        )));
    }
    persist_project_assignments(pool, project_id, &resolved.rows).await
}

struct ResolvedAssignmentImport {
    rows: Vec<(i64, Option<String>)>,
    imported_records: usize,
    errors: Vec<AssignmentRecordError>,
}

async fn resolve_project_assignments(
    pool: &SqlitePool,
    project_id: &str,
    assignments: &[Value],
) -> Result<ResolvedAssignmentImport, AssignmentError> {
    let student_rows = sqlx::query_as::<_, (i64, String)>(
        "SELECT student_id, external_id FROM project_students WHERE project_id=?",
    )
    .bind(project_id)
    .fetch_all(pool)
    .await?;
    let mut student_ids_by_external_id = HashMap::<String, Vec<i64>>::new();
    for (student_id, external_id) in &student_rows {
        student_ids_by_external_id
            .entry(external_id.trim().to_owned())
            .or_default()
            .push(*student_id);
    }

    let delivery_rows = sqlx::query_as::<_, (String, String)>(
        "SELECT d.delivery_id, d.original_filename FROM project_deliveries pd JOIN vm_activity_deliveries d ON d.delivery_id=pd.delivery_id WHERE pd.project_id=?",
    )
    .bind(project_id)
    .fetch_all(pool)
    .await?;
    let mut delivery_ids_by_filename = HashMap::<String, Vec<String>>::new();
    for (delivery_id, filename) in delivery_rows {
        delivery_ids_by_filename
            .entry(filename)
            .or_default()
            .push(delivery_id);
    }

    let mut seen_students = HashSet::new();
    let mut resolved = Vec::with_capacity(assignments.len());
    let mut errors = Vec::new();
    for (index, assignment) in assignments.iter().enumerate() {
        let Some(object) = assignment.as_object() else {
            errors.push(AssignmentRecordError {
                record_index: Some(index + 1),
                student_external_id: None,
                error: "assignment record must be an object".into(),
                correction:
                    "use a mapping with `student_external_id` and `delivery_filename` fields."
                        .into(),
            });
            continue;
        };
        let external_id = match object.get("student_external_id") {
            Some(Value::String(value)) if !value.trim().is_empty() => value.trim(),
            Some(Value::String(_)) | None | Some(Value::Null) => {
                errors.push(AssignmentRecordError {
                    record_index: Some(index + 1),
                    student_external_id: None,
                    error: "student_external_id is missing or empty".into(),
                    correction: "set it to the student's external ID shown by `thot get students`."
                        .into(),
                });
                continue;
            }
            Some(_) => {
                errors.push(AssignmentRecordError {
                    record_index: Some(index + 1),
                    student_external_id: None,
                    error: "student_external_id must be a string".into(),
                    correction: "quote the student's external ID as a string, as shown by `thot get students`.".into(),
                });
                continue;
            }
        };
        let Some(student_ids) = student_ids_by_external_id.get(external_id) else {
            errors.push(AssignmentRecordError {
                record_index: Some(index + 1),
                student_external_id: Some(external_id.to_owned()),
                error: format!(
                    "student_external_id {external_id:?} does not exist in this project"
                ),
                correction: "copy the exact external ID from `thot get students` for this project."
                    .into(),
            });
            continue;
        };
        if student_ids.len() != 1 {
            errors.push(AssignmentRecordError {
                record_index: Some(index + 1),
                student_external_id: Some(external_id.to_owned()),
                error: format!("student_external_id {external_id:?} is not unique in this project"),
                correction:
                    "make external IDs unique in the project student roster, then import again."
                        .into(),
            });
            continue;
        }
        let student_id = student_ids[0];
        if !seen_students.insert(student_id) {
            errors.push(AssignmentRecordError {
                record_index: Some(index + 1),
                student_external_id: Some(external_id.to_owned()),
                error: format!("student_external_id {external_id:?} appears more than once"),
                correction: "keep exactly one assignment row for each student external ID.".into(),
            });
            continue;
        }

        let delivery_filename = match object.get("delivery_filename") {
            None | Some(Value::Null) => None,
            Some(Value::String(filename)) => Some(filename.as_str()),
            Some(_) => {
                errors.push(AssignmentRecordError {
                    record_index: Some(index + 1),
                    student_external_id: Some(external_id.to_owned()),
                    error: "delivery_filename must be a string or null".into(),
                    correction:
                        "use an exact filename from `thot assignments get`, or set it to null."
                            .into(),
                });
                continue;
            }
        };
        let delivery_id = match delivery_filename {
            None => None,
            Some(filename) if filename.trim().is_empty() => {
                errors.push(AssignmentRecordError {
                    record_index: Some(index + 1),
                    student_external_id: Some(external_id.to_owned()),
                    error: "delivery_filename is empty".into(),
                    correction:
                        "use an exact filename from `thot assignments get`, or set it to null."
                            .into(),
                });
                continue;
            }
            Some(filename) => {
                let Some(delivery_ids) = delivery_ids_by_filename.get(filename) else {
                    errors.push(AssignmentRecordError {
                        record_index: Some(index + 1),
                        student_external_id: Some(external_id.to_owned()),
                        error: format!("delivery_filename {filename:?} does not exist in this project"),
                        correction: "copy an exact delivery filename from `thot assignments get`, or use null.".into(),
                    });
                    continue;
                };
                if delivery_ids.len() != 1 {
                    errors.push(AssignmentRecordError {
                        record_index: Some(index + 1),
                        student_external_id: Some(external_id.to_owned()),
                        error: format!(
                            "delivery_filename {filename:?} is not unique in this project"
                        ),
                        correction:
                            "use a unique delivery filename in the project delivery folder.".into(),
                    });
                    continue;
                }
                Some(delivery_ids[0].clone())
            }
        };
        resolved.push((student_id, delivery_id));
    }

    let missing_students = student_rows
        .iter()
        .filter(|(student_id, _)| !seen_students.contains(student_id))
        .map(|(_, external_id)| external_id.clone())
        .collect::<Vec<_>>();
    for external_id in missing_students {
        errors.push(AssignmentRecordError {
            record_index: None,
            student_external_id: Some(external_id.clone()),
            error: format!("project student {external_id:?} has no assignment record"),
            correction: "add one row with this external ID and a delivery filename or null.".into(),
        });
    }

    let imported_records = resolved.len();
    Ok(ResolvedAssignmentImport {
        rows: resolved,
        imported_records,
        errors,
    })
}

async fn persist_project_assignments(
    pool: &SqlitePool,
    project_id: &str,
    assignments: &[(i64, Option<String>)],
) -> Result<(), AssignmentError> {
    let mut transaction = pool.begin().await?;
    sqlx::query("DELETE FROM project_assignments WHERE project_id=?")
        .bind(project_id)
        .execute(&mut *transaction)
        .await?;
    for (student_id, delivery_id) in assignments {
        if let Some(delivery_id) = delivery_id {
            sqlx::query(
                "INSERT INTO project_assignments(project_id, student_id, delivery_id, match_score) VALUES(?, ?, ?, NULL)",
            )
            .bind(project_id)
            .bind(student_id)
            .bind(delivery_id)
            .execute(&mut *transaction)
            .await?;
        }
    }
    transaction.commit().await?;
    Ok(())
}

pub(crate) async fn replace_project_deliveries(
    pool: &SqlitePool,
    project_id: &str,
    deliveries: &[(String, String)],
) -> Result<(), sqlx::Error> {
    let current = deliveries
        .iter()
        .map(|(delivery_id, _)| delivery_id.clone())
        .collect::<HashSet<_>>();
    let previous = sqlx::query_scalar::<_, String>(
        "SELECT delivery_id FROM project_deliveries WHERE project_id=?",
    )
    .bind(project_id)
    .fetch_all(pool)
    .await?;
    let mut transaction = pool.begin().await?;
    for delivery_id in previous {
        if !current.contains(&delivery_id) {
            sqlx::query("DELETE FROM project_assignments WHERE project_id=? AND delivery_id=?")
                .bind(project_id)
                .bind(&delivery_id)
                .execute(&mut *transaction)
                .await?;
            sqlx::query("DELETE FROM project_deliveries WHERE project_id=? AND delivery_id=?")
                .bind(project_id)
                .bind(delivery_id)
                .execute(&mut *transaction)
                .await?;
        }
    }
    for (delivery_id, _) in deliveries {
        sqlx::query(
            "INSERT OR IGNORE INTO project_deliveries(project_id, delivery_id) VALUES(?, ?)",
        )
        .bind(project_id)
        .bind(delivery_id)
        .execute(&mut *transaction)
        .await?;
    }
    transaction.commit().await?;
    Ok(())
}

#[derive(Debug, sqlx::FromRow)]
struct StudentMatchRow {
    id: i64,
    name: String,
    surname: Option<String>,
    email: String,
    external_id: Option<String>,
}

#[derive(Debug, sqlx::FromRow)]
struct DeliveryRow {
    delivery_id: String,
    original_filename: String,
}

#[derive(Debug, sqlx::FromRow)]
struct AssignmentViewRow {
    id: i64,
    name: String,
    surname: Option<String>,
    email: String,
    external_id: Option<String>,
    delivery_id: Option<String>,
    original_filename: Option<String>,
    assignment_count: i64,
    match_score: Option<f64>,
}

#[derive(Debug, sqlx::FromRow)]
struct UnassignedDeliveryRow {
    delivery_id: String,
    original_filename: String,
    assignment_count: i64,
}

fn score_student_filename(student: &StudentMatchRow, filename: &str) -> f64 {
    let filename_tokens = filename_tokens(filename);
    if filename_tokens.is_empty() {
        return 0.0;
    }
    let mut candidate_sets = Vec::new();
    let mut full_name = tokens(&format!(
        "{} {}",
        student.name,
        student.surname.as_deref().unwrap_or_default()
    ));
    full_name.dedup();
    if !full_name.is_empty() {
        candidate_sets.push(full_name);
    }
    let given_name = tokens(&student.name);
    if !given_name.is_empty() {
        candidate_sets.push(given_name);
    }
    if let Some(surname) = student.surname.as_deref() {
        let surname = tokens(surname);
        if !surname.is_empty() {
            candidate_sets.push(surname);
        }
    }
    if let Some((username, _)) = student.email.split_once('@') {
        let username = tokens(username);
        if !username.is_empty() {
            candidate_sets.push(username);
        }
    }
    let email = tokens(&student.email);
    if !email.is_empty() {
        candidate_sets.push(email);
    }
    if let Some(external_id) = student.external_id.as_deref() {
        let external_id = tokens(external_id);
        if !external_id.is_empty() {
            candidate_sets.push(external_id);
        }
    }
    let numeric_id = student.id.to_string();
    if numeric_id.len() >= 3 {
        candidate_sets.push(vec![numeric_id]);
    }
    let mut scores = candidate_sets
        .iter()
        .map(|candidate| token_set_similarity(candidate, &filename_tokens))
        .filter(|score| *score > 0.0)
        .collect::<Vec<_>>();
    scores.sort_by(f64::total_cmp);
    let best = scores.last().copied().unwrap_or(0.0);
    let second = scores.iter().rev().nth(1).copied().unwrap_or(0.0);
    // A second pass rewards filenames that independently resemble more than one student field.
    (0.85 * best + 0.15 * second).min(1.0)
}

fn filename_tokens(filename: &str) -> Vec<String> {
    let stem = Path::new(filename)
        .file_stem()
        .and_then(|value| value.to_str())
        .unwrap_or(filename);
    tokens(stem)
}

fn tokens(value: &str) -> Vec<String> {
    let mut result = Vec::new();
    let mut token = String::new();
    for character in value.chars().flat_map(char::to_lowercase) {
        if character.is_alphanumeric() {
            token.push(character);
        } else if !token.is_empty() {
            result.push(std::mem::take(&mut token));
        }
    }
    if !token.is_empty() {
        result.push(token);
    }
    result
}

fn token_set_similarity(candidate: &[String], filename: &[String]) -> f64 {
    let eligible = candidate
        .iter()
        .filter(|token| token.chars().count() >= 3)
        .collect::<Vec<_>>();
    if eligible.is_empty() {
        return 0.0;
    }
    let matched = eligible
        .iter()
        .filter(|token| {
            filename
                .iter()
                .any(|filename_token| token_similarity(token, filename_token) >= 0.68)
        })
        .count();
    let coverage = matched as f64 / eligible.len() as f64;
    let precision = matched as f64 / filename.len().max(1) as f64;
    0.7 * coverage + 0.3 * precision
}

fn token_similarity(left: &str, right: &str) -> f64 {
    if left == right {
        return 1.0;
    }
    let shorter = if left.chars().count() <= right.chars().count() {
        left
    } else {
        right
    };
    let longer = if shorter == left { right } else { left };
    if shorter.chars().count() >= 4 && longer.contains(shorter) {
        return 0.9;
    }
    let distance = levenshtein(left, right);
    1.0 - distance as f64 / left.chars().count().max(right.chars().count()) as f64
}

fn levenshtein(left: &str, right: &str) -> usize {
    let right = right.chars().collect::<Vec<_>>();
    let mut previous = (0..=right.len()).collect::<Vec<_>>();
    for (left_index, left_char) in left.chars().enumerate() {
        let mut current = Vec::with_capacity(right.len() + 1);
        current.push(left_index + 1);
        for (right_index, right_char) in right.iter().enumerate() {
            let substitution_cost = usize::from(left_char != *right_char);
            current.push(
                (previous[right_index + 1] + 1)
                    .min(current[right_index] + 1)
                    .min(previous[right_index] + substitution_cost),
            );
        }
        previous = current;
    }
    previous[right.len()]
}
