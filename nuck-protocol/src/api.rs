//! `/_nuck/v1` request/response DTOs (ADR 0013).
//!
//! Draft surface — see `docs/protocol/api.md`. These are the shared shapes between
//! nuck-server and the Nuck client; the full endpoint inventory and error model are
//! still being specified.

use serde::{Deserialize, Serialize};

use crate::events::ChannelKind;
use crate::permissions::Permissions;

/// `GET /_nuck/v1/server-info` — capability and version advertisement (ADR 0010/0026).
///
/// Public (unauthenticated). Peers and clients use it to negotiate feature gates.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct ServerInfo {
    /// The server's Matrix `server_name`.
    pub server_name: String,
    /// Product version of nuck-server (moves freely).
    pub product_version: String,
    /// The Nuck protocol version this server speaks.
    pub protocol_version: u32,
    /// The oldest protocol version this server still speaks.
    pub protocol_min_version: u32,
    /// Advertised capabilities, e.g. `guilds`, `voice`, `bots`, `search`.
    pub capabilities: Vec<String>,
}

/// The uniform API error body (ADR 0013/0026: never silently misbehave).
#[derive(Clone, Debug, Deserialize, Serialize, thiserror::Error)]
#[error("{errcode}: {error}")]
pub struct ApiError {
    /// A stable, machine-readable error code, e.g. `M_FORBIDDEN`.
    pub errcode: String,
    /// A human-readable message.
    pub error: String,
}

impl ApiError {
    #[must_use]
    pub fn new(errcode: impl Into<String>, error: impl Into<String>) -> Self {
        Self { errcode: errcode.into(), error: error.into() }
    }
}

// --- Privileged guild actions (ADR 0006/0041) ------------------------------
//
// These are the requests nuck-server authorizes and then executes through the
// privileged **action bot** (ADR 0041). The client never writes privileged guild
// state directly; the resulting substrate event (and the audit entry) carry the
// acting user for attribution.

/// `POST /_nuck/v1/guilds/{guildId}/roles` — create a role.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct CreateRoleRequest {
    pub name: String,
    /// RGB color, `0xRRGGBB`.
    #[serde(default)]
    pub color: Option<u32>,
    /// Higher position wins on conflict. Must be below the caller's highest role.
    pub position: i64,
    pub permissions: Permissions,
    #[serde(default)]
    pub hoist: bool,
    #[serde(default)]
    pub mentionable: bool,
}

/// The created role.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct CreateRoleResponse {
    /// The generated role id (the state key of `space.nuck.role.v1`).
    pub role_id: String,
    /// The substrate event the action bot wrote.
    pub event_id: String,
}

/// `PUT /_nuck/v1/guilds/{guildId}/members/{userId}/roles` — assign a member's roles.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct SetMemberRolesRequest {
    /// The full role set to assign.
    pub roles: Vec<String>,
}

/// `PUT /_nuck/v1/guilds/{guildId}/bans/{userId}` — ban a member (ADR 0016).
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct BanRequest {
    #[serde(default)]
    pub reason: Option<String>,
}

/// The outcome of a privileged action: the substrate event the bot wrote (ADR 0041).
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct ActionResponse {
    pub event_id: String,
}

/// `PUT /_nuck/v1/channels/{channelId}/overrides/{target}` — set a channel override
/// (ADR 0020).
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct SetChannelOverrideRequest {
    /// Bits this override grants.
    #[serde(default)]
    pub allow: Permissions,
    /// Bits this override removes. Deny wins over allow within one override.
    #[serde(default)]
    pub deny: Permissions,
}

/// One channel permission override.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct ChannelOverride {
    /// A role id (including `@everyone`) or a Matrix user id.
    pub target: String,
    pub allow: Permissions,
    pub deny: Permissions,
}

/// `GET /_nuck/v1/channels/{channelId}/overrides` — a channel's overrides (ADR 0020).
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct ChannelOverridesResponse {
    pub overrides: Vec<ChannelOverride>,
}

// --- Guild & channel provisioning (MVP) ------------------------------------

/// `POST /_nuck/v1/guilds` — create a guild.
#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct CreateGuildRequest {
    #[serde(default)]
    pub name: Option<String>,
}

/// A newly created guild and its default channel.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct GuildResponse {
    pub guild_id: String,
    pub channel_id: String,
}

/// `POST /_nuck/v1/guilds/{guildId}/channels` — create a channel.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct CreateChannelRequest {
    pub name: String,
    #[serde(default = "default_channel_kind")]
    pub kind: ChannelKind,
    #[serde(default)]
    pub category: Option<String>,
    #[serde(default)]
    pub position: Option<i64>,
}

fn default_channel_kind() -> ChannelKind {
    ChannelKind::Text
}

/// A channel as the guild view reports it.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct ChannelInfo {
    pub channel_id: String,
    pub kind: String,
    pub position: i64,
    pub category: Option<String>,
}

/// A role as the guild view reports it.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct RoleInfo {
    pub role_id: String,
    pub name: Option<String>,
    pub position: i64,
    pub permissions: Permissions,
    pub color: Option<u32>,
    pub hoist: bool,
    pub mentionable: bool,
}

/// `GET /_nuck/v1/guilds/{guildId}` — the aggregated guild view.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct GuildView {
    pub guild_id: String,
    pub name: Option<String>,
    pub home_server: String,
    pub owner: Option<String>,
    pub channels: Vec<ChannelInfo>,
    pub roles: Vec<RoleInfo>,
}
