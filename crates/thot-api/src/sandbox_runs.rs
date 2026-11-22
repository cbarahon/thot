use axum::{
    Json, Router,
    extract::{Path, State},
    http::{HeaderValue, StatusCode, header},
    response::{IntoResponse, Response},
    routing::{get, post},
};
use serde::Deserialize;
use serde_json::json;
use thot_core::SandboxRunRequest;
use thot_engine::{SandboxRunService, RunRecord, sandbox_run_service::ServiceError};

pub(super) fn router(service: SandboxRunService) -> Router {
    Router::new()
        .route("/api/v2/runs", post(create_run))
        .route("/api/v2/runs/{run_id}", get(get_run))
        .route("/api/v2/runs/{run_id}/cancel", post(cancel_run))
        .with_state(service)
}

async fn create_run(
    State(service): State<SandboxRunService>,
    Json(request): Json<SandboxRunRequest>,
) -> Result<impl IntoResponse, ApiError> {
    service
        .start_run(&request.project_path)
        .await
        .map(|run| (StatusCode::ACCEPTED, Json(run)))
        .map_err(ApiError::from)
}

async fn get_run(
    State(service): State<SandboxRunService>,
    Path(run_id): Path<String>,
) -> Result<Json<RunRecord>, ApiError> {
    service
        .get_run(&run_id)
        .await?
        .map(Json)
        .ok_or_else(|| ApiError(StatusCode::NOT_FOUND, "run not found".into()))
}

async fn cancel_run(
    State(service): State<SandboxRunService>,
    Path(run_id): Path<String>,
) -> Result<Json<RunRecord>, ApiError> {
    service
        .cancel_run(&run_id)
        .await?
        .map(Json)
        .ok_or_else(|| ApiError(StatusCode::NOT_FOUND, "run not found".into()))
}

struct ApiError(StatusCode, String);
impl From<ServiceError> for ApiError {
    fn from(error: ServiceError) -> Self {
        match error {
            ServiceError::Invalid(_) => Self(StatusCode::BAD_REQUEST, "invalid project/input".to_string()),
            ServiceError::Conflict(_) => Self(StatusCode::CONFLICT, "conflict".to_string()),
            ServiceError::StorageError(_) | ServiceError::ProviderError(_) => {
                Self(StatusCode::INTERNAL_SERVER_ERROR, "internal error".to_string())
            }
        }
    }
}