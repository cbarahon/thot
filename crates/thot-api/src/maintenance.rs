//! Purpose: Exposes cleanup, prune, and reset operations over the maintenance API.
use axum::{Json, Router, extract::State, http::StatusCode, response::IntoResponse, routing::post};
use serde_json::json;
use thot_engine::{
    DeliveryRunService, delivery_run_service::ServiceError, maintenance::MaintenanceReport,
};

pub(super) fn router(service: DeliveryRunService) -> Router {
    Router::new()
        .route("/api/v1/maintenance/clean", post(clean))
        .route("/api/v1/maintenance/prune", post(prune))
        .route("/api/v1/maintenance/reset", post(reset))
        .with_state(service)
}

async fn clean(
    State(service): State<DeliveryRunService>,
) -> Result<Json<MaintenanceReport>, ApiError> {
    service.clean().await.map(Json).map_err(ApiError::from)
}

async fn prune(
    State(service): State<DeliveryRunService>,
) -> Result<Json<MaintenanceReport>, ApiError> {
    service.prune().await.map(Json).map_err(ApiError::from)
}

async fn reset(
    State(service): State<DeliveryRunService>,
) -> Result<Json<MaintenanceReport>, ApiError> {
    service.reset().await.map(Json).map_err(ApiError::from)
}

struct ApiError(StatusCode, &'static str);

impl From<ServiceError> for ApiError {
    fn from(error: ServiceError) -> Self {
        match error {
            ServiceError::Conflict(_) => Self(
                StatusCode::CONFLICT,
                "maintenance conflicts with an active run",
            ),
            _ => Self(
                StatusCode::INTERNAL_SERVER_ERROR,
                "internal maintenance error",
            ),
        }
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> axum::response::Response {
        (self.0, Json(json!({"error": self.1}))).into_response()
    }
}
