//! The HTTP surface (ADR 0013).
//!
//! Three namespaces coexist:
//! - the **proxied Matrix C-S API** for the substrate (not yet wired up),
//! - the **Nuck surface** at `/_nuck/v1/*`, whose privileged actions are authorized by
//!   the permission engine and executed by the action bot (ADR 0041), and
//! - the Matrix **AppService** API, the projection's input (ADR 0019).

pub mod appservice;
pub mod auth;
pub mod channels;
pub mod error;
pub mod guilds;
pub mod health;
pub mod privileged;
pub mod proxy;
pub mod server_info;

use std::sync::Arc;

use axum::routing::{any, get, post, put};
use axum::Router;
use tower_http::trace::TraceLayer;

use crate::state::AppState;

/// Build the application router.
pub fn router(state: Arc<AppState>) -> Router {
    Router::new()
        .route("/healthz", get(health::healthz))
        .route("/_nuck/v1/server-info", get(server_info::server_info))
        // Privileged guild actions: authorized here, executed by the action bot
        // (ADR 0041).
        .route("/_nuck/v1/guilds/{guild_id}/roles", post(guilds::create_role))
        .route(
            "/_nuck/v1/guilds/{guild_id}/members/{user_id}/roles",
            put(guilds::set_member_roles),
        )
        .route("/_nuck/v1/guilds/{guild_id}/bans/{user_id}", put(guilds::ban_member))
        // Guild & channel provisioning (MVP M3).
        .route("/_nuck/v1/guilds", post(guilds::create_guild))
        .route("/_nuck/v1/guilds/{guild_id}", get(guilds::get_guild))
        .route(
            "/_nuck/v1/guilds/{guild_id}/channels",
            post(guilds::create_channel),
        )
        .route(
            "/_nuck/v1/channels/{channel_id}/overrides",
            get(channels::list_overrides),
        )
        .route(
            "/_nuck/v1/channels/{channel_id}/overrides/{target}",
            put(channels::set_override).delete(channels::remove_override),
        )
        // Permission-checked send (M4): more specific than the C-S catch-all.
        .route(
            "/_matrix/client/v3/rooms/{room_id}/send/{event_type}/{txn_id}",
            put(proxy::send),
        )
        // The proxied Matrix C-S and media APIs (ADR 0013). The client only talks to
        // nuck-server; the substrate is never exposed.
        .route("/_matrix/client/{*rest}", any(proxy::client))
        .route("/_matrix/media/{*rest}", any(proxy::media))
        // Matrix AppService API — the projection's input (ADR 0019).
        .route("/_matrix/app/v1/transactions/{txn_id}", put(appservice::transaction))
        .route("/_matrix/app/v1/users/{user_id}", get(appservice::user))
        .route("/_matrix/app/v1/rooms/{alias}", get(appservice::room))
        .layer(TraceLayer::new_for_http())
        .with_state(state)
}
