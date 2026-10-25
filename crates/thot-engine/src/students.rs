//! Purpose: Validates, imports, and retrieves student records through the repository interface.
use thot_core::{
    GetStudentsResponse, ImportStudentsRequest, ImportStudentsResponse, RejectedStudent,
    StudentInput,
};

use crate::SqliteStudentRepository;

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
    ) -> Result<ImportStudentsResponse, sqlx::Error> {
        let mut imported = 0;
        let mut rejected = Vec::new();

        for record in request.students {
            if let Some(reason) = validate(&record) {
                rejected.push(rejection(record, reason));
                continue;
            }

            match self.repository.insert(&record).await {
                Ok(_) => imported += 1,
                Err(sqlx::Error::Database(db_err)) if db_err.is_unique_violation() => {
                    rejected.push(rejection(
                        record,
                        "Ya existe un estudiante con este correo electrónico".into(),
                    ));
                }
                Err(error) => return Err(error),
            }
        }

        let discarded = rejected.len();
        Ok(ImportStudentsResponse {
            imported,
            discarded,
            rejected,
        })
    }

    pub async fn list(&self) -> Result<GetStudentsResponse, sqlx::Error> {
        let students = self.repository.list().await?;
        let total = students.len();
        Ok(GetStudentsResponse { students, total })
    }
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
