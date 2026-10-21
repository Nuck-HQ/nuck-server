//! `GET /_nuck/v1/server-info` — capability and version advertisement
//! (ADR 0010/0026).

use std::sync::Arc;

use axum::{extract::State, Json};
use nuck_protocol::api::ServerInfo;
use nuck_protocol::{PROTOCOL_MIN_VERSION, PROTOCOL_VERSION};

use crate::state::AppState;

/// Advertise this server's protocol version and capabilities. Public.
pub async fn server_info(State(state): State<Arc<AppState>>) -> Json<ServerInfo> {
    Json(ServerInfo {
        server_name: state.config.server_name.clone(),
        product_version: env!("CARGO_PKG_VERSION").to_owned(),
        protocol_version: PROTOCOL_VERSION,
        protocol_min_version: PROTOCOL_MIN_VERSION,
        // The v1 capability vocabulary (docs/protocol/api.md). Absent = unsupported.
        // Deferred and therefore NOT advertised: voice, bots, directory, search,
        // federation.
        capabilities: [
            "guilds",
            "channels",
            "roles",
            "members",
            "bans",
            "audit_log",
            "invites",
            "join_requests",
            "registration",
            "friends",
            "blocking",
            "direct_messages",
        ]
        .iter()
        .map(|capability| (*capability).to_owned())
        .collect(),
    })
}
