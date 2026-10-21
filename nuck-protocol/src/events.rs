//! The custom `space.nuck.*` event registry (ADR 0014).
//!
//! These are ordinary Matrix state / message-like / to-device events with a Nuck
//! event type, so they federate via the substrate (ADR 0002) and are enforced by
//! room auth where relevant (ADR 0006).
//!
//! **Versioning:** the version is in the type name. Additive changes keep the type
//! and rely on tolerant parsing (unknown fields are ignored); breaking changes add a
//! new type (`…v2`) and never mutate an existing one.

use ruma::OwnedUserId;
use ruma::events::EmptyStateKey;
use ruma::events::macros::EventContent;
use serde::{Deserialize, Serialize};

use crate::permissions::Permissions;

/// String constants for every registered event type, for matching against raw
/// events in the projection (ADR 0019).
pub mod event_types {
    pub const GUILD: &str = "space.nuck.guild.v1";
    pub const ROLE: &str = "space.nuck.role.v1";
    pub const MEMBER: &str = "space.nuck.member.v1";
    pub const CHANNEL: &str = "space.nuck.channel.v1";
    pub const OVERRIDE: &str = "space.nuck.override.v1";
    pub const INVITE: &str = "space.nuck.invite.v1";
    pub const BAN: &str = "space.nuck.ban.v1";
    pub const JOIN_REQUEST: &str = "space.nuck.join_request.v1";
    pub const EMOJI: &str = "space.nuck.emoji.v1";
    pub const STICKER: &str = "space.nuck.sticker.v1";
    pub const EMBED: &str = "space.nuck.embed.v1";
    pub const INTERACTION: &str = "space.nuck.interaction.v1";
    pub const FRIEND_REQUEST: &str = "space.nuck.friend_request.v1";
    pub const FRIEND_RESPONSE: &str = "space.nuck.friend_response.v1";
}

// --- Enums ------------------------------------------------------------------

/// How a guild may be joined (ADR 0025).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum JoinMode {
    Open,
    InviteOnly,
    Approval,
}

/// Whether a guild appears in the public directory (ADR 0021).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DirectoryVisibility {
    Hidden,
    Listed,
}

/// A guild's default notification level, mapping onto Matrix push rules (ADR 0031).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum NotificationLevel {
    AllMessages,
    MentionsOnly,
    Nothing,
}

/// A channel's type (ADR 0005).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ChannelKind {
    Text,
    Voice,
    Category,
}

// --- Guild room state (ADR 0005) -------------------------------------------

/// `space.nuck.guild.v1` — guild settings (singleton state).
#[derive(Clone, Debug, Deserialize, Serialize, EventContent)]
#[ruma_event(type = "space.nuck.guild.v1", kind = State, state_key_type = EmptyStateKey)]
pub struct GuildSettingsEventContent {
    pub join_mode: JoinMode,
    pub directory_visibility: DirectoryVisibility,
    pub default_notification_level: NotificationLevel,
}

/// `space.nuck.role.v1` — a role definition, keyed by role ID (ADR 0006/0020).
#[derive(Clone, Debug, Deserialize, Serialize, EventContent)]
#[ruma_event(type = "space.nuck.role.v1", kind = State, state_key_type = String)]
pub struct RoleEventContent {
    pub name: String,
    /// RGB color, `0xRRGGBB`.
    pub color: Option<u32>,
    /// Higher position wins on conflict.
    pub position: i64,
    pub permissions: Permissions,
    pub hoist: bool,
    pub mentionable: bool,
}

/// `space.nuck.member.v1` — a member's role assignments, keyed by user ID.
///
/// Kept separate from `m.room.member` so role changes don't rewrite membership.
/// Must be cleaned up when a member leaves.
#[derive(Clone, Debug, Deserialize, Serialize, EventContent)]
#[ruma_event(type = "space.nuck.member.v1", kind = State, state_key_type = OwnedUserId)]
pub struct MemberEventContent {
    pub roles: Vec<String>,
}

/// `space.nuck.channel.v1` — channel structure, keyed by the channel room ID.
#[derive(Clone, Debug, Deserialize, Serialize, EventContent)]
#[ruma_event(type = "space.nuck.channel.v1", kind = State, state_key_type = String)]
pub struct ChannelEventContent {
    pub kind: ChannelKind,
    pub position: i64,
    /// Parent category channel ID, if any.
    pub category: Option<String>,
}

/// `space.nuck.invite.v1` — an invite code, keyed by code hash (ADR 0011).
#[derive(Clone, Debug, Deserialize, Serialize, EventContent)]
#[ruma_event(type = "space.nuck.invite.v1", kind = State, state_key_type = String)]
pub struct InviteEventContent {
    pub max_uses: Option<u32>,
    pub uses: u32,
    /// Milliseconds since the Unix epoch.
    pub expires_at: Option<u64>,
    pub created_by: OwnedUserId,
}

/// `space.nuck.ban.v1` — a federated guild ban, keyed by user ID (ADR 0016).
#[derive(Clone, Debug, Deserialize, Serialize, EventContent)]
#[ruma_event(type = "space.nuck.ban.v1", kind = State, state_key_type = OwnedUserId)]
pub struct BanEventContent {
    pub reason: Option<String>,
    pub banned_by: OwnedUserId,
    pub created_at: u64,
}

/// `space.nuck.join_request.v1` — a pending/decided join request (ADR 0025).
#[derive(Clone, Debug, Deserialize, Serialize, EventContent)]
#[ruma_event(type = "space.nuck.join_request.v1", kind = State, state_key_type = OwnedUserId)]
pub struct JoinRequestEventContent {
    pub message: Option<String>,
    pub status: JoinRequestStatus,
    pub requested_at: u64,
}

/// The state of a join request.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum JoinRequestStatus {
    Pending,
    Approved,
    Denied,
}

// --- Channel room state (ADR 0005/0020) ------------------------------------

/// `space.nuck.override.v1` — a per-channel permission override, keyed by role ID
/// or user ID (ADR 0020).
#[derive(Clone, Debug, Deserialize, Serialize, EventContent)]
#[ruma_event(type = "space.nuck.override.v1", kind = State, state_key_type = String)]
pub struct OverrideEventContent {
    pub allow: Permissions,
    pub deny: Permissions,
}

// --- Message-like events (ADR 0023) ----------------------------------------

/// `space.nuck.embed.v1` — a rich embed.
#[derive(Clone, Debug, Deserialize, Serialize, EventContent)]
#[ruma_event(type = "space.nuck.embed.v1", kind = MessageLike)]
pub struct EmbedEventContent {
    pub title: Option<String>,
    pub description: Option<String>,
    pub url: Option<String>,
    /// RGB color, `0xRRGGBB`.
    pub color: Option<u32>,
}

// --- To-device events (ADR 0030) -------------------------------------------

/// `space.nuck.friend_request.v1` — a friend request, delivered point-to-point.
#[derive(Clone, Debug, Deserialize, Serialize, EventContent)]
#[ruma_event(type = "space.nuck.friend_request.v1", kind = ToDevice)]
pub struct FriendRequestEventContent {
    pub message: Option<String>,
}

/// `space.nuck.friend_response.v1` — accept/decline a friend request.
#[derive(Clone, Debug, Deserialize, Serialize, EventContent)]
#[ruma_event(type = "space.nuck.friend_response.v1", kind = ToDevice)]
pub struct FriendResponseEventContent {
    pub accepted: bool,
}
