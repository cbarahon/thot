//! Adaptador HTTP: las rutas delegan toda la lógica de negocio en StudentService.

use axum::{
    Json, Router,
    extract::State,
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::{get, post},
};
use serde_json::json;
use thot_core::ImportStudentsRequest;
use thot_engine::StudentService;

#[derive(Clone)]
struct ApiState {
    students: StudentService,
}

pub fn router(students: StudentService) -> Router {
    Router::new()
        .route("/health", get(health))
        .route("/import/students", post(import_students))
        .route("/get/students", get(get_students))
        .with_state(ApiState { students })
}

async fn health() -> Json<serde_json::Value> {
    Json(json!({ "status": "ok" }))
}

async fn import_students(
    State(state): State<ApiState>,
    Json(request): Json<ImportStudentsRequest>,
) -> Result<impl IntoResponse, ApiError> {
    let response = state
        .students
        .import(request)
        .await
        .map_err(ApiError::from)?;
    Ok(Json(response))
}

async fn get_students(State(state): State<ApiState>) -> Result<impl IntoResponse, ApiError> {
    let response = state.students.list().await.map_err(ApiError::from)?;
    Ok(Json(response))
}

struct ApiError(String);

impl From<sqlx::Error> for ApiError {
    fn from(error: sqlx::Error) -> Self {
        tracing::error!(error = %error, "fallo en la capa de persistencia");
        Self("error interno de almacenamiento".into())
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": self.0 })),
        )
            .into_response()
    }
}
