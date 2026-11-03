use axum::{extract::State, http::StatusCode, Json};
use serde::{Deserialize, Serialize};

use crate::config::Settings;

#[derive(Debug, Serialize, Deserialize)]
pub struct HealthResponse {
    pub status: &'static str,
    pub service: &'static str,
}

pub async fn health_check(State(_settings): State<Settings>) -> (StatusCode, Json<HealthResponse>) {
    (
        StatusCode::OK,
        Json(HealthResponse {
            status: "ok",
            service: "finance-management-api",
        }),
    )
}
