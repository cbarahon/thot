//! Purpose: Exposes automatic assignment guessing, assignment retrieval, and replacement imports.
use axum::{
    Json, Router,
    extract::{Query, State},
    http::StatusCode,
    response::IntoResponse,
    routing::{get, post},
};
use serde::Deserialize;
use serde_json::json;
use thot_engine::{
    AssignmentService,
    assignments::{
        AssignmentError, AssignmentImportResponse, AssignmentsResponse, ImportAssignmentsRequest,
    },
    project_context::ProjectContextError,
};

pub(super) fn router(service: AssignmentService) -> Router {
    Router::new()
        .route("/api/v1/assignments/guess", post(guess))
        .route("/api/v1/assignments/get", get(get_assignments))
        .route("/api/v1/assignments/import", post(import_assignments))
        .with_state(service)
}

#[derive(Debug, Default, Deserialize)]
struct ProjectPathQuery {
    project_path: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
struct ProjectPathBody {
    project_path: Option<String>,
}

async fn guess(
    State(service): State<AssignmentService>,
    body: Option<Json<ProjectPathBody>>,
) -> Result<Json<AssignmentsResponse>, ApiError> {
    let project_path = body.and_then(|Json(body)| body.project_path);
    service
        .guess(project_path.as_deref())
        .await
        .map(Json)
        .map_err(ApiError::from)
}

async fn get_assignments(
    State(service): State<AssignmentService>,
    Query(query): Query<ProjectPathQuery>,
) -> Result<Json<AssignmentsResponse>, ApiError> {
    service
        .get(query.project_path.as_deref())
        .await
        .map(Json)
        .map_err(ApiError::from)
}

async fn import_assignments(
    State(service): State<AssignmentService>,
    Json(request): Json<ImportAssignmentsRequest>,
) -> Result<Json<AssignmentImportResponse>, ApiError> {
    service
        .import(request)
        .await
        .map(Json)
        .map_err(ApiError::from)
}

struct ApiError(StatusCode, String);

impl From<AssignmentError> for ApiError {
    fn from(error: AssignmentError) -> Self {
        match error {
            AssignmentError::Invalid(message) => Self(StatusCode::BAD_REQUEST, message),
            AssignmentError::Project(ProjectContextError::Invalid(message)) => {
                Self(StatusCode::BAD_REQUEST, message)
            }
            AssignmentError::Project(ProjectContextError::Storage(error)) => {
                tracing::error!(%error, "assignment project context storage failure");
                Self(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "internal assignment storage error".into(),
                )
            }
            AssignmentError::Storage(error) => {
                tracing::error!(%error, "assignment storage failure");
                Self(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "internal assignment storage error".into(),
                )
            }
        }
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> axum::response::Response {
        (self.0, Json(json!({"error": self.1}))).into_response()
    }
}
