//! Shared plumbing for privileged actions (ADR 0016/0041).
//!
//! Every privileged handler: loads the caller's standing from the projection,
//! enforces the home-server boundary, resolves the action bot, and records an audit
//! entry. Keeping it here means guild and channel handlers cannot drift apart.

use std::time::{SystemTime, UNIX_EPOCH};

use nuck_protocol::permissions::Permissions;

use crate::authorization::{self, GuildAccess};
use crate::bot::ActionBot;
use crate::projection::{AuditEntry, Projection};
use crate::state::AppState;

use super::error::ApiErrorResponse;

/// Load the caller's standing in a guild, mapping "unknown guild" to 404.
pub async fn load(
    state: &AppState,
    guild_id: &str,
    actor: &str,
) -> Result<GuildAccess, ApiErrorResponse> {
    authorization::load(projection(state)?, guild_id, actor)
        .await
        .map_err(internal)?
        .ok_or_else(|| ApiErrorResponse::not_found("unknown guild"))
}

/// Only the guild's home server is authoritative for its privileged actions
/// (ADR 0038/0041). Relaying to the home server is not implemented yet.
pub fn require_local(state: &AppState, access: &GuildAccess) -> Result<(), ApiErrorResponse> {
    // A local room id may omit the `:server` part (Tuwunel does), so an empty home
    // server means "this server". Remote rooms always carry their server.
    if !access.guild.home_server.is_empty() && access.guild.home_server != state.config.server_name
    {
        return Err(ApiErrorResponse::not_implemented(
            "this guild is hosted on another server; cross-server relay is not implemented yet",
        ));
    }
    Ok(())
}

/// Require a permission, or 403 with a message naming it.
pub fn require(
    access: &GuildAccess,
    permission: Permissions,
    name: &str,
) -> Result<(), ApiErrorResponse> {
    if access.has(permission) {
        Ok(())
    } else {
        Err(ApiErrorResponse::forbidden(format!("{name} is required")))
    }
}

/// Require the caller to be a **joined** member of the guild (ADR 0005).
///
/// Membership is the substrate-authoritative gate; holding a stale role assignment is
/// not the same as being in the guild.
pub fn require_member(access: &GuildAccess) -> Result<(), ApiErrorResponse> {
    if access.is_member() {
        Ok(())
    } else {
        Err(ApiErrorResponse::forbidden("you are not a member of this guild"))
    }
}

/// A user's membership state in a room, if projected.
pub async fn membership(
    state: &AppState,
    room_id: &str,
    user_id: &str,
) -> Result<Option<String>, ApiErrorResponse> {
    Ok(projection(state)?
        .get_membership(room_id, user_id)
        .await
        .map_err(internal)?
        .map(|membership| membership.membership))
}

/// Resolve the action bot from configuration (ADR 0041). Errors as 503 when the
/// deployment has no bot, rather than silently doing nothing.
pub fn action_bot(state: &AppState) -> Result<ActionBot<'_>, ApiErrorResponse> {
    let user = state
        .config
        .bot_user
        .as_deref()
        .ok_or_else(|| ApiErrorResponse::unavailable("the action bot is not configured"))?;
    let token = state
        .config
        .bot_token
        .as_deref()
        .or(state.config.appservice_as_token.as_deref())
        .ok_or_else(|| ApiErrorResponse::unavailable("the action bot has no AppService token"))?;
    Ok(ActionBot::new(&state.substrate, user, token))
}

/// The projection store, or 503 when the server runs without one.
pub fn projection(state: &AppState) -> Result<&Projection, ApiErrorResponse> {
    state
        .projection
        .as_ref()
        .ok_or_else(|| ApiErrorResponse::unavailable("projection store is not configured"))
}

/// A member's recorded role assignments (`[]` when there is no assignment row).
pub async fn member_roles(
    state: &AppState,
    guild_id: &str,
    user_id: &str,
) -> Result<Vec<String>, ApiErrorResponse> {
    Ok(projection(state)?
        .get_member(guild_id, user_id)
        .await
        .map_err(internal)?
        .map(|member| member.roles)
        .unwrap_or_default())
}

/// Append the audit entry that attributes a privileged action to its actor (ADR 0016).
pub async fn record(
    state: &AppState,
    guild_id: &str,
    actor: &str,
    action: &str,
    target: Option<&str>,
    event_id: &str,
) -> Result<(), ApiErrorResponse> {
    let entry = AuditEntry {
        guild_id: guild_id.to_owned(),
        actor: actor.to_owned(),
        action: action.to_owned(),
        target: target.map(ToOwned::to_owned),
        event_id: event_id.to_owned(),
        created_at: now_ms(),
    };
    projection(state)?.append_audit(&entry).await.map_err(internal)
}

/// Milliseconds since the Unix epoch.
pub fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|elapsed| elapsed.as_millis() as i64)
        .unwrap_or_default()
}

/// A fresh opaque role id: `r` + 12 lowercase hex characters (ADR 0042).
///
/// Opaque ids avoid leaking names or positions, stay stable across renames, and cannot
/// be mistaken for a Matrix user id. See `docs/protocol/events.md`.
pub fn new_role_id() -> String {
    let random = uuid::Uuid::new_v4().simple().to_string();
    format!("r{}", &random[..12])
}

/// Map any error into a 500 response.
pub fn internal<E: std::fmt::Display>(e: E) -> ApiErrorResponse {
    ApiErrorResponse::internal(e.to_string())
}
