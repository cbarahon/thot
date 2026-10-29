//! Purpose: Builds the HTTP router for student and delivery-run API endpoints.

use axum::{
    Json, Router,
    extract::{Query, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::get,
};
use serde::Deserialize;
use serde_json::json;
use thot_core::ImportStudentsRequest;
use thot_engine::project_context::ProjectContextError;
use thot_engine::{AssignmentService, DeliveryRunService, StudentService, StudentServiceError};

mod assignments;
mod delivery_runs;
mod maintenance;

#[derive(Clone)]
struct StudentState {
    students: StudentService,
}

#[derive(Debug, Default, Deserialize)]
struct ProjectPathQuery {
    project_path: Option<String>,
}

pub fn router(students: StudentService) -> Router {
    Router::new()
        .route("/health", get(health))
        .route("/import/students", axum::routing::post(import_students))
        .route("/get/students", get(get_students))
        .with_state(StudentState { students })
}

pub fn router_with_runs(
    students: StudentService,
    runs: DeliveryRunService,
    assignments_service: AssignmentService,
) -> Router {
    router(students)
        .merge(delivery_runs::router(runs.clone()))
        .merge(maintenance::router(runs))
        .merge(assignments::router(assignments_service))
}

async fn health() -> Json<serde_json::Value> {
    Json(json!({"status":"ok"}))
}

async fn import_students(
    State(state): State<StudentState>,
    Json(request): Json<ImportStudentsRequest>,
) -> Result<impl IntoResponse, ApiError> {
    state
        .students
        .import(request)
        .await
        .map(Json)
        .map_err(ApiError::from)
}

async fn get_students(
    State(state): State<StudentState>,
    Query(query): Query<ProjectPathQuery>,
) -> Result<impl IntoResponse, ApiError> {
    state
        .students
        .list(query.project_path.as_deref())
        .await
        .map(Json)
        .map_err(ApiError::from)
}

pub(crate) struct ApiError(String, StatusCode);
impl From<sqlx::Error> for ApiError {
    fn from(error: sqlx::Error) -> Self {
        tracing::error!(%error, "database request failed");
        Self(
            "internal storage error".into(),
            StatusCode::INTERNAL_SERVER_ERROR,
        )
    }
}
impl From<StudentServiceError> for ApiError {
    fn from(error: StudentServiceError) -> Self {
        match error {
            StudentServiceError::Project(ProjectContextError::Invalid(message)) => {
                Self(message, StatusCode::BAD_REQUEST)
            }
            StudentServiceError::Project(ProjectContextError::Storage(error))
            | StudentServiceError::Storage(error) => {
                tracing::error!(%error, "student service storage failed");
                Self(
                    "internal storage error".into(),
                    StatusCode::INTERNAL_SERVER_ERROR,
                )
            }
        }
    }
}
impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        (self.1, Json(json!({"error": self.0}))).into_response()
    }
}
