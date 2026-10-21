//! Matrix AppService endpoints (ADR 0019).
//!
//! Tuwunel pushes **every event in rooms the projector user is joined to** —
//! including federated events — to `PUT /_matrix/app/v1/transactions/{txn_id}`. This
//! is the projection's input, and the reason the projector user must be auto-joined
//! to every guild room.
//!
//! The two query endpoints answer the homeserver's namespace checks.

use std::sync::Arc;

use axum::extract::{Path, State};
use axum::http::{HeaderMap, StatusCode, header};
use axum::response::IntoResponse;
use axum::{Json, response::Response};
use serde_json::json;

use crate::projection::{Transaction, ingest};
use crate::state::AppState;

use super::error::ApiErrorResponse;

/// `PUT /_matrix/app/v1/transactions/{txn_id}` — receive a batch of events.
///
/// Idempotent: an event already ingested is skipped, so redelivery is harmless
/// (ADR 0019).
pub async fn transaction(
    State(state): State<Arc<AppState>>,
    Path(_txn_id): Path<String>,
    headers: HeaderMap,
    Json(txn): Json<Transaction>,
) -> Result<Json<serde_json::Value>, ApiErrorResponse> {
    authorize(&state, &headers)?;

    let Some(projection) = &state.projection else {
        return Err(ApiErrorResponse::unavailable("projection store is not configured"));
    };

    for event in &txn.events {
        let is_new = projection
            .record_event(&event.event_id, &event.room_id)
            .await
            .map_err(|e| ApiErrorResponse::internal(e.to_string()))?;
        if !is_new {
            continue;
        }

        match ingest::project(event) {
            Ok(Some(op)) => {
                if let Err(e) = projection.apply(op).await {
                    tracing::error!(event_id = %event.event_id, "projection apply failed: {e}");
                    return Err(ApiErrorResponse::internal(e.to_string()));
                }
            }
            // Not a projected event type — ignored for forward compatibility (ADR 0014).
            Ok(None) => {}
            Err(e) => {
                tracing::warn!(
                    event_id = %event.event_id,
                    event_type = %event.event_type,
                    "skipping malformed event: {e}"
                );
            }
        }
    }

    Ok(Json(json!({})))
}

/// `GET /_matrix/app/v1/users/{user_id}` — does this user belong to the AppService?
pub async fn user(State(state): State<Arc<AppState>>, Path(user_id): Path<String>) -> Response {
    let claimed = state.config.appservice_user.as_deref() == Some(user_id.as_str());
    let status = if claimed { StatusCode::OK } else { StatusCode::NOT_FOUND };
    (status, Json(json!({}))).into_response()
}

/// `GET /_matrix/app/v1/rooms/{alias}` — does this alias belong to the AppService?
pub async fn room() -> Response {
    (StatusCode::NOT_FOUND, Json(json!({}))).into_response()
}

/// Verify the `hs_token` the homeserver presents on every push (ADR 0028).
fn authorize(state: &AppState, headers: &HeaderMap) -> Result<(), ApiErrorResponse> {
    let Some(expected) = &state.config.appservice_hs_token else {
        // No token configured: development mode only.
        return Ok(());
    };

    let presented = headers.get(header::AUTHORIZATION).and_then(|v| v.to_str().ok());
    match presented {
        Some(value) if value == format!("Bearer {expected}") => Ok(()),
        _ => Err(ApiErrorResponse::unauthorized()),
    }
}
