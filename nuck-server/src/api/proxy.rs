//! The proxied Matrix Client-Server API (ADR 0013/0028).
//!
//! The Nuck client only ever talks to nuck-server (ADR 0001); the substrate is never
//! internet-facing. This forwards the C-S and media APIs to Tuwunel, preserving the
//! request exactly (including the access token) and streaming the response back.
//!
//! The one interception is `PUT .../rooms/{id}/send/...` (M4): the request is
//! authorized by the permission engine before it is forwarded, so guild-channel sends
//! obey roles and channel overrides. Rooms the projection does not know (e.g. a DM)
//! pass through — the substrate's coarse power levels still apply.

use std::sync::Arc;

use axum::body::Body;
use axum::extract::{Path, Request, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};

use nuck_protocol::permissions::Permissions;

use crate::authorization;
use crate::state::AppState;

use super::auth::AuthenticatedUser;
use super::error::ApiErrorResponse;

/// `/_matrix/client/{*rest}` — transparent C-S proxy.
pub async fn client(State(state): State<Arc<AppState>>, req: Request<Body>) -> Response {
    forward(&state, req).await
}

/// `/_matrix/media/{*rest}` — transparent media proxy.
pub async fn media(State(state): State<Arc<AppState>>, req: Request<Body>) -> Response {
    forward(&state, req).await
}

/// `PUT /_matrix/client/v3/rooms/{roomId}/send/{eventType}/{txnId}` — authorize a send
/// against guild roles and channel overrides, then forward it (M4).
pub async fn send(
    State(state): State<Arc<AppState>>,
    AuthenticatedUser { user_id, .. }: AuthenticatedUser,
    Path((room_id, _event_type, _txn_id)): Path<(String, String, String)>,
    req: Request<Body>,
) -> Response {
    if let Err(e) = authorize_send(&state, &user_id, &room_id).await {
        return e.into_response();
    }
    forward(&state, req).await
}

/// Check `SEND_MESSAGES` for a known guild channel. Unknown rooms are forwarded so
/// non-guild traffic (DMs, joins) is not broken by the MVP's guild-only model.
async fn authorize_send(
    state: &AppState,
    user_id: &str,
    room_id: &str,
) -> Result<(), ApiErrorResponse> {
    let Some(projection) = state.projection.as_ref() else {
        return Ok(());
    };
    let Some(channel) = projection
        .get_channel(room_id)
        .await
        .map_err(internal)?
    else {
        return Ok(());
    };

    let membership = projection
        .get_membership(room_id, user_id)
        .await
        .map_err(internal)?
        .map(|membership| membership.membership);
    if membership.as_deref() != Some("join") {
        return Err(ApiErrorResponse::forbidden(
            "you are not a member of this channel",
        ));
    }

    let access = authorization::load(projection, &channel.guild_id, user_id)
        .await
        .map_err(internal)?
        .ok_or_else(|| ApiErrorResponse::forbidden("unknown guild"))?;
    let overrides = authorization::overrides_from_rows(
        &projection
            .list_overrides(room_id)
            .await
            .map_err(internal)?,
    );
    let parsed = ruma::UserId::parse(user_id).map_err(|_| ApiErrorResponse::unauthorized())?;
    let permissions = access.channel_permissions(&parsed, &overrides);
    if !permissions.contains(Permissions::SEND_MESSAGES) {
        return Err(ApiErrorResponse::forbidden("SEND_MESSAGES is required"));
    }
    Ok(())
}

/// Forward a request to Tuwunel, streaming the body both ways.
async fn forward(state: &AppState, req: Request<Body>) -> Response {
    let (parts, body) = req.into_parts();

    let path = parts.uri.path_and_query().map_or("/", |pq| pq.as_str());
    let url = format!("{}{path}", state.substrate.base_url());
    let method = reqwest::Method::from_bytes(parts.method.as_str().as_bytes())
        .unwrap_or(reqwest::Method::GET);

    let mut builder = state.substrate.http().request(method, &url);
    for (name, value) in &parts.headers {
        if hop_by_hop(name.as_str()) {
            continue;
        }
        builder = builder.header(name.clone(), value.clone());
    }
    let request = builder.body(reqwest::Body::wrap_stream(body.into_data_stream()));

    let response = match request.send().await {
        Ok(response) => response,
        // A substrate outage is a gateway failure, not the client's fault.
        Err(e) => {
            tracing::warn!("proxy request to {url} failed: {e}");
            return (StatusCode::BAD_GATEWAY, "the homeserver is unavailable").into_response();
        }
    };

    let status = StatusCode::from_u16(response.status().as_u16())
        .unwrap_or(StatusCode::BAD_GATEWAY);
    let mut out = Response::builder().status(status);
    for (name, value) in response.headers() {
        if hop_by_hop(name.as_str()) {
            continue;
        }
        out = out.header(name.clone(), value.clone());
    }
    out.body(Body::from_stream(response.bytes_stream()))
        .unwrap_or_else(|_| StatusCode::BAD_GATEWAY.into_response())
}

/// Headers that must not be forwarded between two HTTP hops.
fn hop_by_hop(name: &str) -> bool {
    matches!(
        name.to_ascii_lowercase().as_str(),
        "connection"
            | "keep-alive"
            | "proxy-authenticate"
            | "proxy-authorization"
            | "te"
            | "trailer"
            | "transfer-encoding"
            | "upgrade"
            | "host"
            | "content-length"
    )
}

fn internal<E: std::fmt::Display>(e: E) -> ApiErrorResponse {
    ApiErrorResponse::internal(e.to_string())
}
