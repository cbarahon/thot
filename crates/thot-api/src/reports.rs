//! Purpose: Exposes JSON run, delivery, evaluation-statistics, and expedient-matrix reports.
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
        .route("/api/v1/reports/test_stats", get(test_stats))
        .route("/api/v1/reports/checks_stats", get(checks_stats))
        .route("/api/v1/reports/rubric_stats", get(rubric_stats))
        .route("/api/v1/reports/expedients", get(expedients))
        .with_state(service)
}

#[derive(Debug, Default, Deserialize)]
struct DeliveryReportQuery {
    run_id: Option<String>,
    view: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
struct RunReportQuery {
    run_id: Option<String>,
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

async fn test_stats(
    State(service): State<DeliveryRunService>,
    Query(query): Query<RunReportQuery>,
) -> Result<Json<Value>, ApiError> {
    service
        .report_test_stats(query.run_id.as_deref())
        .await
        .map(Json)
        .map_err(ApiError::from)
}

async fn checks_stats(
    State(service): State<DeliveryRunService>,
    Query(query): Query<RunReportQuery>,
) -> Result<Json<Value>, ApiError> {
    service
        .report_checks_stats(query.run_id.as_deref())
        .await
        .map(Json)
        .map_err(ApiError::from)
}

async fn rubric_stats(
    State(service): State<DeliveryRunService>,
    Query(query): Query<RunReportQuery>,
) -> Result<Json<Value>, ApiError> {
    service
        .report_rubric_stats(query.run_id.as_deref())
        .await
        .map(Json)
        .map_err(ApiError::from)
}

async fn expedients(
    State(service): State<DeliveryRunService>,
    Query(query): Query<RunReportQuery>,
) -> Result<Json<Value>, ApiError> {
    service
        .report_expedients(query.run_id.as_deref())
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
