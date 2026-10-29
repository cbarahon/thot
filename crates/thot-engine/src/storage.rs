//! Purpose: Implements SQLite persistence for student records.
use sqlx::{
    SqlitePool,
    sqlite::{SqliteConnectOptions, SqlitePoolOptions},
};
use std::{path::Path, str::FromStr};

use crate::EngineError;
use thot_core::{Student, StudentInput};

#[derive(Clone)]
pub struct SqliteStudentRepository {
    pool: SqlitePool,
}

impl SqliteStudentRepository {
    pub async fn connect(database_url: &str) -> Result<Self, EngineError> {
        if let Some(path) = database_url.strip_prefix("sqlite:") {
            let path = path.split('?').next().unwrap_or(path);
            if path != ":memory:" {
                if let Some(parent) = Path::new(path).parent() {
                    if !parent.as_os_str().is_empty() {
                        std::fs::create_dir_all(parent)
                            .map_err(|e| EngineError::StorageInit(e.to_string()))?;
                    }
                }
            }
        }

        let options = SqliteConnectOptions::from_str(database_url)
            .map_err(|e| EngineError::StorageInit(e.to_string()))?
            .create_if_missing(true)
            .foreign_keys(true);

        let pool = SqlitePoolOptions::new()
            .max_connections(5)
            .connect_with(options)
            .await?;

        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS students (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                name TEXT NOT NULL,
                surname TEXT,
                email TEXT NOT NULL COLLATE NOCASE UNIQUE,
                external_id TEXT,
                group_name TEXT,
                center TEXT,
                birth_date TEXT,
                created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
            )
            "#,
        )
        .execute(&pool)
        .await?;

        Ok(Self { pool })
    }

    pub async fn insert(&self, input: &StudentInput) -> Result<Student, sqlx::Error> {
        let result = sqlx::query(
            r#"
            INSERT INTO students
                (name, surname, email, external_id, group_name, center, birth_date)
            VALUES (?, ?, ?, ?, ?, ?, ?)
            "#,
        )
        .bind(input.name.trim())
        .bind(clean_optional(input.surname.as_deref()))
        .bind(input.email.trim())
        .bind(clean_optional(input.external_id.as_deref()))
        .bind(clean_optional(input.group_name.as_deref()))
        .bind(clean_optional(input.center.as_deref()))
        .bind(clean_optional(input.birth_date.as_deref()))
        .execute(&self.pool)
        .await?;

        Ok(Student {
            id: result.last_insert_rowid(),
            name: input.name.trim().to_owned(),
            surname: clean_optional(input.surname.as_deref()),
            email: input.email.trim().to_owned(),
            external_id: clean_optional(input.external_id.as_deref()),
            group_name: clean_optional(input.group_name.as_deref()),
            center: clean_optional(input.center.as_deref()),
            birth_date: clean_optional(input.birth_date.as_deref()),
        })
    }

    pub(crate) fn pool(&self) -> &SqlitePool {
        &self.pool
    }

    pub async fn list_for_project(&self, project_id: &str) -> Result<Vec<Student>, sqlx::Error> {
        let rows = sqlx::query_as::<_, StudentRow>(
            r#"
            SELECT s.id, ps.name, ps.surname, ps.email, ps.external_id,
                   ps.group_name, ps.center, ps.birth_date
            FROM students s
            JOIN project_students ps ON ps.student_id=s.id
            WHERE ps.project_id=?
            ORDER BY s.id
            "#,
        )
        .bind(project_id)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows.into_iter().map(Into::into).collect())
    }

    pub async fn list(&self) -> Result<Vec<Student>, sqlx::Error> {
        let rows = sqlx::query_as::<_, StudentRow>(
            r#"
            SELECT id, name, surname, email, external_id, group_name, center, birth_date
            FROM students ORDER BY id
            "#,
        )
        .fetch_all(&self.pool)
        .await?;

        Ok(rows.into_iter().map(Into::into).collect())
    }
}

fn clean_optional(value: Option<&str>) -> Option<String> {
    value
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(ToOwned::to_owned)
}

#[derive(sqlx::FromRow)]
struct StudentRow {
    id: i64,
    name: String,
    surname: Option<String>,
    email: String,
    external_id: Option<String>,
    group_name: Option<String>,
    center: Option<String>,
    birth_date: Option<String>,
}

impl From<StudentRow> for Student {
    fn from(row: StudentRow) -> Self {
        Self {
            id: row.id,
            name: row.name,
            surname: row.surname,
            email: row.email,
            external_id: row.external_id,
            group_name: row.group_name,
            center: row.center,
            birth_date: row.birth_date,
        }
    }
}
