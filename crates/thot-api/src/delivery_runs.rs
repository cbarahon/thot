//! Purpose: Defines HTTP handlers for creating and managing delivery assessment runs.
use axum::{
    Json, Router,
    extract::{Path, State},
    http::StatusCode,
    response::IntoResponse,
    routing::{get, post},
};
use serde::Deserialize;
use serde_json::json;
use thot_core::DeliveryRunRequest;
use thot_engine::{DeliveryRunService, RunRecord, delivery_run_service::ServiceError};

pub(super) fn router(service: DeliveryRunService) -> Router {
    Router::new()
        .route("/api/v1/runs", get(list_runs).post(create_run))
        .route("/api/v1/runs/checks", post(run_checks))
        .route("/api/v1/runs/{run_id}", get(get_run).delete(delete_run))
        .route("/api/v1/runs/{run_id}/cancel", post(cancel_run))
        .route("/api/v1/expedients/{expedient_id}", get(get_expedient))
        .route(
            "/api/v1/expedients/{expedient_id}/cleanup",
            post(cleanup_expedient),
        )
        .with_state(service)
}

async fn list_runs(
    State(service): State<DeliveryRunService>,
) -> Result<Json<Vec<RunRecord>>, ApiError> {
    service.list_runs().await.map(Json).map_err(ApiError::from)
}

#[derive(Debug, Default, Deserialize)]
struct RunChecksRequest {
    run_id: Option<String>,
    project_path: Option<String>,
}

async fn run_checks(
    State(service): State<DeliveryRunService>,
    body: Option<Json<RunChecksRequest>>,
) -> Result<impl IntoResponse, ApiError> {
    let (run_id, project_path) = body
        .map(|Json(request)| (request.run_id, request.project_path))
        .unwrap_or_default();
    service
        .run_checks(run_id.as_deref(), project_path.as_deref())
        .await?
        .map(|result| (StatusCode::OK, Json(result)))
        .ok_or_else(|| ApiError(StatusCode::NOT_FOUND, "run not found".into()))
}

async fn create_run(
    State(service): State<DeliveryRunService>,
    Json(request): Json<DeliveryRunRequest>,
) -> Result<impl IntoResponse, ApiError> {
    service
        .create_run(request.project_path, request.delivery_directory)
        .await
        .map(|run| (StatusCode::ACCEPTED, Json(run)))
        .map_err(ApiError::from)
}

async fn get_run(
    State(service): State<DeliveryRunService>,
    Path(run_id): Path<String>,
) -> Result<Json<RunRecord>, ApiError> {
    service
        .get_run(&run_id)
        .await?
        .map(Json)
        .ok_or_else(|| ApiError(StatusCode::NOT_FOUND, "run not found".into()))
}

async fn cancel_run(
    State(service): State<DeliveryRunService>,
    Path(run_id): Path<String>,
) -> Result<Json<RunRecord>, ApiError> {
    service
        .cancel_run(&run_id)
        .await?
        .map(Json)
        .ok_or_else(|| ApiError(StatusCode::NOT_FOUND, "run not found".into()))
}

async fn delete_run(
    State(service): State<DeliveryRunService>,
    Path(run_id): Path<String>,
) -> Result<Json<serde_json::Value>, ApiError> {
    if service.delete_run(&run_id).await? {
        Ok(Json(json!({"run_id": run_id, "deleted": true})))
    } else {
        Err(ApiError(StatusCode::NOT_FOUND, "run not found".into()))
    }
}

async fn get_expedient(
    State(service): State<DeliveryRunService>,
    Path(id): Path<String>,
) -> Result<Json<serde_json::Value>, ApiError> {
    service
        .get_expedient(&id)
        .await?
        .map(Json)
        .ok_or_else(|| ApiError(StatusCode::NOT_FOUND, "expedient not found".into()))
}

async fn cleanup_expedient(
    State(service): State<DeliveryRunService>,
    Path(id): Path<String>,
) -> Result<Json<serde_json::Value>, ApiError> {
    service
        .cleanup_expedient(&id)
        .await?
        .map(Json)
        .ok_or_else(|| {
            ApiError(
                StatusCode::NOT_FOUND,
                "expedient not found or its report is unavailable".into(),
            )
        })
}

struct ApiError(StatusCode, String);
impl From<ServiceError> for ApiError {
    fn from(error: ServiceError) -> Self {
        match error {
            ServiceError::Invalid(message) => Self(StatusCode::BAD_REQUEST, message),
            ServiceError::Conflict(message) => Self(StatusCode::CONFLICT, message),
            ServiceError::Internal(message) => Self(StatusCode::INTERNAL_SERVER_ERROR, message),
            ServiceError::Storage(error) => {
                tracing::error!(%error, "delivery storage failure");
                Self(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "internal storage error".into(),
                )
            }
        }
    }
}
impl axum::response::IntoResponse for ApiError {
    fn into_response(self) -> axum::response::Response {
        (self.0, Json(json!({"error":self.1}))).into_response()
    }
}
