//! Purpose: Builds the HTTP router for student and delivery-run API endpoints.

use axum::{
    Json, Router,
    extract::State,
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::get,
};
use serde_json::json;
use thot_core::ImportStudentsRequest;
use thot_engine::{DeliveryRunService, StudentService};

mod delivery_runs;
mod maintenance;

#[derive(Clone)]
struct StudentState {
    students: StudentService,
}

pub fn router(students: StudentService) -> Router {
    Router::new()
        .route("/health", get(health))
        .route("/import/students", axum::routing::post(import_students))
        .route("/get/students", get(get_students))
        .with_state(StudentState { students })
}

pub fn router_with_runs(students: StudentService, runs: DeliveryRunService) -> Router {
    router(students)
        .merge(delivery_runs::router(runs.clone()))
        .merge(maintenance::router(runs))
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

async fn get_students(State(state): State<StudentState>) -> Result<impl IntoResponse, ApiError> {
    state
        .students
        .list()
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
impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        (self.1, Json(json!({"error": self.0}))).into_response()
    }
}
