//! Mapping from raw Matrix events to projection operations (ADR 0019).
//!
//! This is deliberately **pure**: given a raw event it returns the operation to apply,
//! with no I/O. That keeps the mapping — the part where a schema slip corrupts state —
//! unit-testable without a database.
//!
//! Ingestion is driven by the AppService transaction push (see `crate::api::appservice`).

use anyhow::{Context, Result};
use nuck_protocol::events::event_types as et;
use nuck_protocol::events::{
    BanEventContent, ChannelEventContent, GuildSettingsEventContent, InviteEventContent,
    JoinRequestEventContent, MemberEventContent, OverrideEventContent, RoleEventContent,
};
use serde::{Deserialize, Serialize};

pub use crate::matrix::RawEvent;

use super::ProjectionOp;
use super::models::{
    BanRow, ChannelRow, GuildOwnerRow, GuildRow, InviteRow, JoinRequestRow, MemberRow,
    MembershipRow, MessageRow, OverrideRow, RoleRow,
};

/// An AppService transaction body.
#[derive(Debug, Clone, Deserialize)]
pub struct Transaction {
    /// The events pushed by the homeserver.
    #[serde(default)]
    pub events: Vec<RawEvent>,
}

/// Map a raw event to a projection operation, or `None` if it is not projected.
///
/// Unknown event types are ignored (forward compatibility, ADR 0014); a *known* type
/// with malformed content is an error.
pub fn project(event: &RawEvent) -> Result<Option<ProjectionOp>> {
    let op = match event.event_type.as_str() {
        et::GUILD => {
            if is_removal(event) {
                ProjectionOp::RemoveGuild(event.room_id.clone())
            } else {
                let c: GuildSettingsEventContent = content(event)?;
                ProjectionOp::Guild(GuildRow {
                    guild_id: event.room_id.clone(),
                    home_server: server_of(&event.room_id),
                    name: None,
                    join_mode: Some(enum_str(&c.join_mode)),
                    directory_visibility: Some(enum_str(&c.directory_visibility)),
                    default_notification_level: Some(enum_str(&c.default_notification_level)),
                    event_id: event.event_id.clone(),
                })
            }
        }
        et::ROLE => {
            if is_removal(event) {
                ProjectionOp::RemoveRole {
                    guild_id: event.room_id.clone(),
                    role_id: event.state_key().to_owned(),
                }
            } else {
                let c: RoleEventContent = content(event)?;
                ProjectionOp::Role(RoleRow {
                    guild_id: event.room_id.clone(),
                    role_id: event.state_key().to_owned(),
                    position: c.position,
                    permissions: c.permissions.bits() as i64,
                    name: Some(c.name),
                    color: c.color.map(i64::from),
                    hoist: c.hoist,
                    mentionable: c.mentionable,
                    event_id: event.event_id.clone(),
                })
            }
        }
        et::MEMBER => {
            if is_removal(event) {
                ProjectionOp::RemoveMember {
                    guild_id: event.room_id.clone(),
                    user_id: crate::keys::decode_user_key(event.state_key()),
                }
            } else {
                let c: MemberEventContent = content(event)?;
                ProjectionOp::Member(MemberRow {
                    guild_id: event.room_id.clone(),
                    user_id: crate::keys::decode_user_key(event.state_key()),
                    roles: c.roles,
                    event_id: event.event_id.clone(),
                })
            }
        }
        et::CHANNEL => {
            if is_removal(event) {
                ProjectionOp::RemoveChannel { channel_id: event.state_key().to_owned() }
            } else {
                let c: ChannelEventContent = content(event)?;
                ProjectionOp::Channel(ChannelRow {
                    channel_id: event.state_key().to_owned(),
                    guild_id: event.room_id.clone(),
                    kind: enum_str(&c.kind),
                    position: c.position,
                    category: c.category,
                    event_id: event.event_id.clone(),
                })
            }
        }
        et::OVERRIDE => {
            let c: OverrideEventContent = content(event)?;
            ProjectionOp::Override(OverrideRow {
                channel_id: event.room_id.clone(),
                target: crate::keys::decode_override_target(event.state_key()),
                allow: c.allow.bits() as i64,
                deny: c.deny.bits() as i64,
                event_id: event.event_id.clone(),
            })
        }
        et::BAN => {
            if is_removal(event) {
                ProjectionOp::RemoveBan {
                    guild_id: event.room_id.clone(),
                    user_id: crate::keys::decode_user_key(event.state_key()),
                }
            } else {
                let c: BanEventContent = content(event)?;
                ProjectionOp::Ban(BanRow {
                    guild_id: event.room_id.clone(),
                    user_id: crate::keys::decode_user_key(event.state_key()),
                    reason: c.reason,
                    banned_by: Some(c.banned_by.to_string()),
                    created_at: Some(c.created_at as i64),
                    event_id: event.event_id.clone(),
                })
            }
        }
        et::INVITE => {
            if is_removal(event) {
                ProjectionOp::RemoveInvite {
                    guild_id: event.room_id.clone(),
                    code_hash: event.state_key().to_owned(),
                }
            } else {
                let c: InviteEventContent = content(event)?;
                ProjectionOp::Invite(InviteRow {
                    guild_id: event.room_id.clone(),
                    code_hash: event.state_key().to_owned(),
                    // The plaintext code is local-only; federated state carries the hash.
                    code: None,
                    max_uses: c.max_uses.map(|u| u as i32),
                    uses: c.uses as i32,
                    expires_at: c.expires_at.map(|e| e as i64),
                    created_by: Some(c.created_by.to_string()),
                    event_id: event.event_id.clone(),
                })
            }
        }
        et::JOIN_REQUEST => {
            if is_removal(event) {
                ProjectionOp::RemoveJoinRequest {
                    guild_id: event.room_id.clone(),
                    user_id: crate::keys::decode_user_key(event.state_key()),
                }
            } else {
                let c: JoinRequestEventContent = content(event)?;
                ProjectionOp::JoinRequest(JoinRequestRow {
                    guild_id: event.room_id.clone(),
                    user_id: crate::keys::decode_user_key(event.state_key()),
                    message: c.message,
                    status: enum_str(&c.status),
                    requested_at: c.requested_at as i64,
                    event_id: event.event_id.clone(),
                })
            }
        }
        // Guild ownership: only Matrix Spaces can be guilds (ADR 0005), and the owner
        // is the room creator (ADR 0020/0038).
        "m.room.create" => {
            if event.content.get("type").and_then(|t| t.as_str()) != Some("m.space") {
                return Ok(None);
            }
            // Room v11+ dropped `creator`; the create event's sender is the creator.
            let owner = event
                .content
                .get("creator")
                .and_then(|c| c.as_str())
                .unwrap_or(event.sender.as_str());
            ProjectionOp::GuildOwner(GuildOwnerRow {
                guild_id: event.room_id.clone(),
                owner: owner.to_owned(),
                event_id: event.event_id.clone(),
            })
        }
        // Room membership (ADR 0005): guild membership is membership in the guild
        // Space room, channel access is membership in the channel room. Redacted
        // member events carry no `membership`, so they are ignored rather than
        // rejected (the resolve step keeps the last known state).
        "m.room.member" => match event.content.get("membership").and_then(|m| m.as_str()) {
            Some(membership) => ProjectionOp::Membership(MembershipRow {
                room_id: event.room_id.clone(),
                user_id: event.state_key().to_owned(),
                membership: membership.to_owned(),
                event_id: event.event_id.clone(),
            }),
            None => return Ok(None),
        },
        // Plaintext messages only: E2EE content arrives as `m.room.encrypted`, which
        // has no body and is therefore never indexed (ADR 0008/0029).
        "m.room.message" => match event.content.get("body").and_then(|b| b.as_str()) {
            Some(body) => ProjectionOp::Message(MessageRow {
                event_id: event.event_id.clone(),
                room_id: event.room_id.clone(),
                sender: event.sender.clone(),
                origin_ts: event.origin_server_ts.unwrap_or(0) as i64,
                body: body.to_owned(),
            }),
            None => return Ok(None),
        },
        _ => return Ok(None),
    };

    Ok(Some(op))
}

fn content<T: serde::de::DeserializeOwned>(event: &RawEvent) -> Result<T> {
    serde_json::from_value(event.content.clone())
        .with_context(|| format!("malformed content for {}", event.event_type))
}

/// Whether a state event signals removal: an **empty content object**.
///
/// Matrix has no state deletion, so Nuck encodes removal as empty content — the shape a
/// redaction produces, and the same convention `m.room.member` already relies on. A
/// non-empty but malformed content is still an error (ADR 0014).
fn is_removal(event: &RawEvent) -> bool {
    event.content.as_object().is_some_and(|content| content.is_empty())
}

fn enum_str<T: Serialize>(value: &T) -> String {
    serde_json::to_value(value)
        .ok()
        .and_then(|v| v.as_str().map(ToOwned::to_owned))
        .unwrap_or_default()
}

/// The server part of a Matrix id (`!opaque:server` → `server`).
fn server_of(id: &str) -> String {
    id.split_once(':').map(|(_, s)| s.to_owned()).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    use nuck_protocol::permissions::Permissions;
    use serde_json::json;

    fn event(event_type: &str, state_key: Option<&str>, content: serde_json::Value) -> RawEvent {
        RawEvent {
            event_id: "$e1".to_owned(),
            room_id: "!guild:example.org".to_owned(),
            sender: "@a:example.org".to_owned(),
            event_type: event_type.to_owned(),
            state_key: state_key.map(ToOwned::to_owned),
            origin_server_ts: Some(1_700_000_000_000),
            content,
        }
    }

    #[test]
    fn guild_settings_maps_to_guild() {
        let e = event(
            et::GUILD,
            Some(""),
            json!({ "join_mode": "approval", "directory_visibility": "listed", "default_notification_level": "mentions_only" }),
        );
        match project(&e).unwrap().unwrap() {
            ProjectionOp::Guild(g) => {
                assert_eq!(g.guild_id, "!guild:example.org");
                assert_eq!(g.home_server, "example.org");
                assert_eq!(g.join_mode.as_deref(), Some("approval"));
            }
            other => panic!("expected Guild, got {other:?}"),
        }
    }

    #[test]
    fn role_maps_with_permission_bits() {
        let perms = Permissions::SEND_MESSAGES.union(Permissions::MANAGE_ROLES);
        let e = event(
            et::ROLE,
            Some("mod"),
            json!({ "name": "Mod", "color": 65280, "position": 50, "permissions": perms.bits(), "hoist": true, "mentionable": false }),
        );
        match project(&e).unwrap().unwrap() {
            ProjectionOp::Role(r) => {
                assert_eq!(r.role_id, "mod");
                assert_eq!(r.perms(), perms);
                assert_eq!(r.position, 50);
            }
            other => panic!("expected Role, got {other:?}"),
        }
    }

    #[test]
    fn member_maps_roles_from_state_key() {
        let e = event(et::MEMBER, Some("@u:example.org"), json!({ "roles": ["mod", "vip"] }));
        match project(&e).unwrap().unwrap() {
            ProjectionOp::Member(m) => {
                assert_eq!(m.user_id, "@u:example.org");
                assert_eq!(m.roles, vec!["mod".to_owned(), "vip".to_owned()]);
            }
            other => panic!("expected Member, got {other:?}"),
        }
    }

    #[test]
    fn channel_maps_guild_from_room_and_id_from_state_key() {
        let e = event(et::CHANNEL, Some("!chan:example.org"), json!({ "kind": "voice", "position": 2, "category": null }));
        match project(&e).unwrap().unwrap() {
            ProjectionOp::Channel(c) => {
                assert_eq!(c.channel_id, "!chan:example.org");
                assert_eq!(c.guild_id, "!guild:example.org");
                assert_eq!(c.kind, "voice");
            }
            other => panic!("expected Channel, got {other:?}"),
        }
    }

    #[test]
    fn override_maps_channel_from_room() {
        let e = event(et::OVERRIDE, Some("mod"), json!({ "allow": 0, "deny": 8192 }));
        match project(&e).unwrap().unwrap() {
            ProjectionOp::Override(o) => {
                assert_eq!(o.channel_id, "!guild:example.org");
                assert_eq!(o.target, "mod");
                assert_eq!(o.deny, 8192);
            }
            other => panic!("expected Override, got {other:?}"),
        }
    }

    #[test]
    fn plaintext_message_is_indexed() {
        let e = event("m.room.message", None, json!({ "msgtype": "m.text", "body": "hello" }));
        match project(&e).unwrap().unwrap() {
            ProjectionOp::Message(m) => assert_eq!(m.body, "hello"),
            other => panic!("expected Message, got {other:?}"),
        }
    }

    #[test]
    fn encrypted_message_is_not_indexed() {
        let e = event("m.room.encrypted", None, json!({ "ciphertext": "..." }));
        assert!(project(&e).unwrap().is_none());
    }

    #[test]
    fn room_membership_maps_from_the_state_key() {
        let e = event("m.room.member", Some("@u:example.org"), json!({ "membership": "join" }));
        match project(&e).unwrap().unwrap() {
            ProjectionOp::Membership(m) => {
                assert_eq!(m.user_id, "@u:example.org");
                assert_eq!(m.room_id, "!guild:example.org");
                assert_eq!(m.membership, "join");
            }
            other => panic!("expected Membership, got {other:?}"),
        }
    }

    #[test]
    fn redacted_membership_is_ignored_not_rejected() {
        // A redacted `m.room.member` has empty content; it must not fail the transaction.
        let e = event("m.room.member", Some("@u:example.org"), json!({}));
        assert!(project(&e).unwrap().is_none());
    }

    #[test]
    fn space_creation_maps_to_the_guild_owner() {
        let mut e = event("m.room.create", Some(""), json!({ "type": "m.space", "creator": "@alice:example.org" }));
        e.sender = "@ignored:example.org".to_owned();
        match project(&e).unwrap().unwrap() {
            ProjectionOp::GuildOwner(o) => {
                assert_eq!(o.guild_id, "!guild:example.org");
                assert_eq!(o.owner, "@alice:example.org", "the `creator` field wins");
            }
            other => panic!("expected GuildOwner, got {other:?}"),
        }
    }

    #[test]
    fn space_creation_falls_back_to_the_sender_without_a_creator_field() {
        // Room v11 dropped `creator`; the create event's sender is the owner.
        let e = event("m.room.create", Some(""), json!({ "type": "m.space" }));
        match project(&e).unwrap().unwrap() {
            ProjectionOp::GuildOwner(o) => assert_eq!(o.owner, "@a:example.org"),
            other => panic!("expected GuildOwner, got {other:?}"),
        }
    }

    #[test]
    fn non_space_room_creation_is_not_a_guild_owner() {
        let e = event("m.room.create", Some(""), json!({ "creator": "@alice:example.org" }));
        assert!(project(&e).unwrap().is_none(), "a plain room is not a guild");
    }

    #[test]
    fn unknown_event_type_is_ignored() {
        let e = event("com.example.custom", Some(""), json!({}));
        assert!(project(&e).unwrap().is_none());
    }

    #[test]
    fn malformed_known_event_is_an_error() {
        let e = event(et::ROLE, Some("mod"), json!({ "name": "Mod" })); // missing required fields
        assert!(project(&e).is_err());
    }

    #[test]
    fn empty_content_encodes_removal() {
        // Matrix has no state deletion, so empty content removes (like a redaction).
        assert!(matches!(
            project(&event(et::ROLE, Some("mod"), json!({}))).unwrap().unwrap(),
            ProjectionOp::RemoveRole { .. }
        ));
        assert!(matches!(
            project(&event(et::CHANNEL, Some("!c:example.org"), json!({}))).unwrap().unwrap(),
            ProjectionOp::RemoveChannel { .. }
        ));
        assert!(matches!(
            project(&event(et::MEMBER, Some("@u:example.org"), json!({}))).unwrap().unwrap(),
            ProjectionOp::RemoveMember { .. }
        ));
        assert!(matches!(
            project(&event(et::BAN, Some("@u:example.org"), json!({}))).unwrap().unwrap(),
            ProjectionOp::RemoveBan { .. }
        ));
        assert!(matches!(
            project(&event(et::GUILD, Some(""), json!({}))).unwrap().unwrap(),
            ProjectionOp::RemoveGuild(_)
        ));
    }
}
