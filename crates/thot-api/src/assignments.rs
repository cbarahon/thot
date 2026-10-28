//! Purpose: Exposes automatic assignment guessing, assignment retrieval, and replacement imports.
use axum::{
    Json, Router,
    extract::State,
    http::StatusCode,
    response::IntoResponse,
    routing::{get, post},
};
use serde_json::json;
use thot_engine::{
    AssignmentService,
    assignments::{AssignmentError, AssignmentsResponse, ImportAssignmentsRequest},
};

pub(super) fn router(service: AssignmentService) -> Router {
    Router::new()
        .route("/api/v1/assignments/guess", post(guess))
        .route("/api/v1/assignments/get", get(get_assignments))
        .route("/api/v1/assignments/import", post(import_assignments))
        .with_state(service)
}

async fn guess(
    State(service): State<AssignmentService>,
) -> Result<Json<AssignmentsResponse>, ApiError> {
    service.guess().await.map(Json).map_err(ApiError::from)
}

async fn get_assignments(
    State(service): State<AssignmentService>,
) -> Result<Json<AssignmentsResponse>, ApiError> {
    service.get().await.map(Json).map_err(ApiError::from)
}

async fn import_assignments(
    State(service): State<AssignmentService>,
    Json(request): Json<ImportAssignmentsRequest>,
) -> Result<Json<AssignmentsResponse>, ApiError> {
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
