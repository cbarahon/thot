//! Purpose: Exposes compact JSON reports for saved runs and their deliveries.
use axum::{
    Json, Router,
    extract::{Query, State},
    http::StatusCode,
    response::IntoResponse,
    routing::get,
};
use serde::Deserialize;
use serde_json::{Value, json};
use thot_engine::{DeliveryRunService, delivery_run_service::ServiceError};

pub(super) fn router(service: DeliveryRunService) -> Router {
    Router::new()
        .route("/api/v1/reports/runs", get(runs))
        .route("/api/v1/reports/deliveries", get(deliveries))
        .with_state(service)
}

#[derive(Debug, Default, Deserialize)]
struct DeliveryReportQuery {
    run_id: Option<String>,
    view: Option<String>,
}

async fn runs(State(service): State<DeliveryRunService>) -> Result<Json<Value>, ApiError> {
    service
        .report_runs()
        .await
        .map(Json)
        .map_err(ApiError::from)
}

async fn deliveries(
    State(service): State<DeliveryRunService>,
    Query(query): Query<DeliveryReportQuery>,
) -> Result<Json<Value>, ApiError> {
    service
        .report_deliveries(
            query.run_id.as_deref(),
            query.view.as_deref().unwrap_or("summary"),
        )
        .await
        .map(Json)
        .map_err(ApiError::from)
}

struct ApiError(StatusCode, String);

impl From<ServiceError> for ApiError {
    fn from(error: ServiceError) -> Self {
        match error {
            ServiceError::Invalid(message) => Self(StatusCode::BAD_REQUEST, message),
            ServiceError::Conflict(message) => Self(StatusCode::CONFLICT, message),
            ServiceError::Internal(message) => Self(StatusCode::INTERNAL_SERVER_ERROR, message),
            ServiceError::Storage(error) => {
                tracing::error!(%error, "report storage failure");
                Self(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "internal storage error".into(),
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
