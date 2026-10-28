//! Purpose: Guesses, stores, and reports student-to-delivery assignments.
use serde::{Deserialize, Serialize};
use sqlx::{
    SqlitePool,
    sqlite::{SqliteConnectOptions, SqlitePoolOptions},
};
use std::{collections::HashSet, path::Path, str::FromStr};
use thiserror::Error;

const MINIMUM_MATCH_SCORE: f64 = 0.56;
const AMBIGUOUS_MATCH_MARGIN: f64 = 0.04;

#[derive(Debug, Error)]
pub enum AssignmentError {
    #[error("invalid assignment data: {0}")]
    Invalid(String),
    #[error("assignment storage error: {0}")]
    Storage(#[from] sqlx::Error),
}

#[derive(Debug, Deserialize)]
pub struct ImportAssignmentsRequest {
    #[serde(default)]
    pub assignments: Vec<AssignmentInput>,
}

#[derive(Debug, Deserialize)]
pub struct AssignmentInput {
    pub student_id: i64,
    #[serde(default)]
    pub delivery_id: Option<String>,
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
    pub assignments: Vec<StudentAssignment>,
    pub unassigned_deliverables: Vec<DeliveryAssignmentSummary>,
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
        sqlx::query(
            "CREATE TABLE IF NOT EXISTS student_assignments (student_id INTEGER PRIMARY KEY, delivery_id TEXT NOT NULL, match_score REAL, FOREIGN KEY(student_id) REFERENCES students(id) ON DELETE CASCADE, FOREIGN KEY(delivery_id) REFERENCES vm_activity_deliveries(delivery_id) ON DELETE CASCADE)",
        )
        .execute(&pool)
        .await?;
        Ok(Self { pool })
    }

    /// Recomputes and stores the best non-ambiguous filename match for each student.
    pub async fn guess(&self) -> Result<AssignmentsResponse, AssignmentError> {
        let students = sqlx::query_as::<_, StudentMatchRow>(
            "SELECT id, name, surname, email, external_id FROM students ORDER BY id",
        )
        .fetch_all(&self.pool)
        .await?;
        let deliveries = sqlx::query_as::<_, DeliveryRow>(
            "SELECT delivery_id, original_filename FROM vm_activity_deliveries ORDER BY original_filename, delivery_id",
        )
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
        sqlx::query("DELETE FROM student_assignments")
            .execute(&mut *transaction)
            .await?;
        for (student_id, delivery_id, score) in suggestions {
            sqlx::query(
                "INSERT INTO student_assignments(student_id, delivery_id, match_score) VALUES(?, ?, ?)",
            )
            .bind(student_id)
            .bind(delivery_id)
            .bind(score)
            .execute(&mut *transaction)
            .await?;
        }
        transaction.commit().await?;
        self.get().await
    }

    /// Returns one row per student and the deliveries not assigned to anyone.
    pub async fn get(&self) -> Result<AssignmentsResponse, AssignmentError> {
        let rows = sqlx::query_as::<_, AssignmentViewRow>(
            r#"SELECT s.id, s.name, s.surname, s.email, s.external_id,
                      a.delivery_id, d.original_filename,
                      COALESCE((SELECT COUNT(*) FROM student_assignments counts
                                WHERE counts.delivery_id = a.delivery_id), 0) AS assignment_count,
                      a.match_score
               FROM students s
               LEFT JOIN student_assignments a ON a.student_id = s.id
               LEFT JOIN vm_activity_deliveries d ON d.delivery_id = a.delivery_id
               ORDER BY s.id"#,
        )
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
                      (SELECT COUNT(*) FROM student_assignments a
                       WHERE a.delivery_id = d.delivery_id) AS assignment_count
               FROM vm_activity_deliveries d
               WHERE NOT EXISTS (
                   SELECT 1 FROM student_assignments a WHERE a.delivery_id = d.delivery_id
               )
               ORDER BY d.original_filename, d.delivery_id"#,
        )
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
            assignments,
            unassigned_deliverables,
        })
    }

    /// Replaces all saved assignments after validating every referenced student and delivery.
    pub async fn import(
        &self,
        request: ImportAssignmentsRequest,
    ) -> Result<AssignmentsResponse, AssignmentError> {
        let students = sqlx::query_scalar::<_, i64>("SELECT id FROM students")
            .fetch_all(&self.pool)
            .await?
            .into_iter()
            .collect::<HashSet<_>>();
        let deliveries =
            sqlx::query_scalar::<_, String>("SELECT delivery_id FROM vm_activity_deliveries")
                .fetch_all(&self.pool)
                .await?
                .into_iter()
                .collect::<HashSet<_>>();
        let mut seen_students = HashSet::new();
        for assignment in &request.assignments {
            if !students.contains(&assignment.student_id) {
                return Err(AssignmentError::Invalid(format!(
                    "student_id {} does not exist",
                    assignment.student_id
                )));
            }
            if !seen_students.insert(assignment.student_id) {
                return Err(AssignmentError::Invalid(format!(
                    "student_id {} appears more than once",
                    assignment.student_id
                )));
            }
            if let Some(delivery_id) = assignment.delivery_id.as_ref() {
                if !deliveries.contains(delivery_id) {
                    return Err(AssignmentError::Invalid(format!(
                        "delivery_id {delivery_id:?} does not exist"
                    )));
                }
            }
        }
        let missing_students = students
            .difference(&seen_students)
            .copied()
            .collect::<Vec<_>>();
        if !missing_students.is_empty() {
            return Err(AssignmentError::Invalid(format!(
                "assignment import must include every student; missing student IDs: {}",
                missing_students
                    .iter()
                    .map(ToString::to_string)
                    .collect::<Vec<_>>()
                    .join(", ")
            )));
        }

        let mut transaction = self.pool.begin().await?;
        sqlx::query("DELETE FROM student_assignments")
            .execute(&mut *transaction)
            .await?;
        for assignment in request.assignments {
            if let Some(delivery_id) = assignment.delivery_id {
                sqlx::query(
                    "INSERT INTO student_assignments(student_id, delivery_id, match_score) VALUES(?, ?, NULL)",
                )
                .bind(assignment.student_id)
                .bind(delivery_id)
                .execute(&mut *transaction)
                .await?;
            }
        }
        transaction.commit().await?;
        self.get().await
    }
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
    candidate_sets
        .iter()
        .map(|candidate| token_set_similarity(candidate, &filename_tokens))
        .fold(0.0, f64::max)
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
                .any(|filename_token| token_similarity(token, filename_token) >= 0.78)
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
