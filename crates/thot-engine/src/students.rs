//! Purpose: Validates, imports, and retrieves student records through the repository interface.
use sqlx::SqlitePool;
use thiserror::Error;
use thot_core::{
    GetStudentsResponse, ImportStudentsRequest, ImportStudentsResponse, RejectedStudent,
    StudentInput,
};

use crate::{
    SqliteStudentRepository,
    project_context::{self, ProjectContextError},
};

#[derive(Debug, Error)]
pub enum StudentServiceError {
    #[error(transparent)]
    Project(#[from] ProjectContextError),
    #[error("student storage error: {0}")]
    Storage(#[from] sqlx::Error),
}

#[derive(Clone)]
pub struct StudentService {
    repository: SqliteStudentRepository,
}

impl StudentService {
    pub fn new(repository: SqliteStudentRepository) -> Self {
        Self { repository }
    }

    pub async fn import(
        &self,
        request: ImportStudentsRequest,
    ) -> Result<ImportStudentsResponse, StudentServiceError> {
        let pool = self.repository.pool();
        let project = project_context::resolve(pool, request.project_path.as_deref()).await?;
        let (imported, discarded, rejected) =
            import_project_students(pool, &project.project_id, request.students).await?;
        Ok(ImportStudentsResponse {
            project_id: project.project_id,
            project_name: project.project_name,
            project_path: project.project_path,
            imported,
            discarded,
            rejected,
        })
    }

    pub async fn list(
        &self,
        requested_project_path: Option<&str>,
    ) -> Result<GetStudentsResponse, StudentServiceError> {
        let project =
            project_context::resolve(self.repository.pool(), requested_project_path).await?;
        let students = self
            .repository
            .list_for_project(&project.project_id)
            .await?;
        let total = students.len();
        Ok(GetStudentsResponse {
            project_id: project.project_id,
            project_name: project.project_name,
            project_path: project.project_path,
            students,
            total,
        })
    }
}

pub(crate) async fn import_project_students(
    pool: &SqlitePool,
    project_id: &str,
    records: Vec<StudentInput>,
) -> Result<(usize, usize, Vec<RejectedStudent>), sqlx::Error> {
    let mut imported = 0;
    let mut rejected = Vec::new();
    let mut seen_emails = std::collections::HashSet::new();

    for mut record in records {
        if record
            .external_id
            .as_deref()
            .is_none_or(|external_id| external_id.trim().is_empty())
        {
            record.external_id = Some(record.email.trim().to_owned());
        }
        if let Some(reason) = validate(&record) {
            rejected.push(rejection(record, reason));
            continue;
        }
        let email_key = record.email.trim().to_lowercase();
        if !seen_emails.insert(email_key) {
            rejected.push(rejection(
                record,
                "El correo electrónico está duplicado en el fichero".into(),
            ));
            continue;
        }

        sqlx::query(
            r#"INSERT INTO students
                   (name, surname, email, external_id, group_name, center, birth_date)
               VALUES (?, ?, ?, ?, ?, ?, ?)
               ON CONFLICT(email) DO NOTHING"#,
        )
        .bind(record.name.trim())
        .bind(clean_optional(record.surname.as_deref()))
        .bind(record.email.trim())
        .bind(clean_optional(record.external_id.as_deref()))
        .bind(clean_optional(record.group_name.as_deref()))
        .bind(clean_optional(record.center.as_deref()))
        .bind(clean_optional(record.birth_date.as_deref()))
        .execute(pool)
        .await?;
        let student_id = sqlx::query_scalar::<_, i64>("SELECT id FROM students WHERE email=?")
            .bind(record.email.trim())
            .fetch_one(pool)
            .await?;
        sqlx::query("INSERT INTO project_students(project_id, student_id, name, surname, email, external_id, group_name, center, birth_date) VALUES(?, ?, ?, ?, ?, ?, ?, ?, ?) ON CONFLICT(project_id, student_id) DO UPDATE SET name=excluded.name, surname=excluded.surname, email=excluded.email, external_id=excluded.external_id, group_name=excluded.group_name, center=excluded.center, birth_date=excluded.birth_date")
            .bind(project_id)
            .bind(student_id)
            .bind(record.name.trim())
            .bind(clean_optional(record.surname.as_deref()))
            .bind(record.email.trim())
            .bind(record.external_id.as_deref().unwrap_or(record.email.trim()))
            .bind(clean_optional(record.group_name.as_deref()))
            .bind(clean_optional(record.center.as_deref()))
            .bind(clean_optional(record.birth_date.as_deref()))
            .execute(pool)
            .await?;
        imported += 1;
    }
    let discarded = rejected.len();
    Ok((imported, discarded, rejected))
}

fn clean_optional(value: Option<&str>) -> Option<String> {
    value
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
}

fn validate(record: &StudentInput) -> Option<String> {
    if record.name.trim().is_empty() {
        return Some("El nombre es obligatorio".into());
    }
    let email = record.email.trim();
    if email.is_empty() || email.chars().any(char::is_whitespace) {
        return Some("El correo electrónico no es válido".into());
    }
    let mut parts = email.split('@');
    let local = parts.next().unwrap_or_default();
    let domain = parts.next().unwrap_or_default();
    if local.is_empty()
        || domain.is_empty()
        || parts.next().is_some()
        || !domain.contains('.')
        || domain.starts_with('.')
        || domain.ends_with('.')
    {
        return Some("El correo electrónico no es válido".into());
    }
    if record.birth_date.as_deref().is_some_and(|date| {
        !date.trim().is_empty()
            && (date.trim().len() != 10
                || date.trim().chars().nth(4) != Some('-')
                || date.trim().chars().nth(7) != Some('-'))
    }) {
        return Some("La fecha de nacimiento debe tener formato YYYY-MM-DD".into());
    }
    None
}

fn rejection(record: StudentInput, reason: String) -> RejectedStudent {
    RejectedStudent {
        row: record.source_row,
        record,
        reason,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn input(name: &str, email: &str, row: u64) -> StudentInput {
        StudentInput {
            source_row: Some(row),
            name: name.into(),
            surname: None,
            email: email.into(),
            external_id: None,
            group_name: None,
            center: None,
            birth_date: None,
        }
    }

    #[test]
    fn rejects_empty_name() {
        assert_eq!(
            validate(&input(" ", "a@example.org", 2)).as_deref(),
            Some("El nombre es obligatorio")
        );
    }

    #[test]
    fn rejects_invalid_email() {
        assert_eq!(
            validate(&input("Anna", "not-an-email", 2)).as_deref(),
            Some("El correo electrónico no es válido")
        );
    }

    #[test]
    fn accepts_valid_student() {
        assert_eq!(validate(&input("Anna", "anna@example.org", 2)), None);
    }
}
