//! Server-side authorization (ADR 0020/0041).
//!
//! Every privileged `/_nuck/v1` action runs through this. The caller's identity comes
//! from the substrate ([`crate::api::auth`]); their roles and the guild's roles come
//! from the projection (which mirrors federated `space.nuck.*` state); and the
//! **authoritative** decision is computed here — never by the substrate, whose power
//! levels are only coarse (ADR 0041).
//!
//! The resolution itself lives in [`nuck_protocol::authorization`]; this module is the
//! I/O + bookkeeping around loading it.

use anyhow::Result;
use nuck_protocol::authorization::{
    EVERYONE_ROLE_ID, GuildPermissions, Override, OverrideTarget, Role,
};
use nuck_protocol::permissions::Permissions;
use ruma::UserId;

use crate::projection::{GuildRow, OverrideRow, Projection};

/// A caller's resolved standing in one guild.
pub struct GuildAccess {
    /// The guild itself.
    pub guild: GuildRow,
    /// The guild's role configuration (`@everyone` base + defined roles).
    pub permissions: GuildPermissions,
    /// The roles the caller holds.
    pub actor_roles: Vec<String>,
    /// The caller's highest role position (`@everyone` is 0).
    pub actor_highest: i64,
    /// The caller's membership state in the guild room (`join`, `leave`, `ban`, …),
    /// if projected.
    pub actor_membership: Option<String>,
    /// Whether the caller created the guild (ADR 0020's owner bypass).
    pub actor_is_owner: bool,
}

impl GuildAccess {
    /// Whether the caller is the guild owner (ADR 0020).
    #[must_use]
    pub fn is_owner(&self) -> bool {
        self.actor_is_owner
    }

    /// Whether the caller is a **joined** member of the guild (ADR 0005).
    ///
    /// Guild membership is membership in the guild Space room. An invited, left, or
    /// banned user is not a member even if they hold role assignments.
    #[must_use]
    pub fn is_member(&self) -> bool {
        self.actor_membership.as_deref() == Some("join")
    }

    /// The caller's effective permissions in a channel, folding in their overrides
    /// (ADR 0020). The owner bypasses overrides.
    #[must_use]
    pub fn channel_permissions(&self, user_id: &UserId, overrides: &[Override]) -> Permissions {
        self.permissions
            .channel_permissions(&self.actor_roles, user_id, self.actor_is_owner, overrides)
    }

    /// Whether the caller holds `permission` at the guild level (ADR 0020).
    #[must_use]
    pub fn has(&self, permission: Permissions) -> bool {
        self.permissions.has_permission(&self.actor_roles, permission)
    }

    /// Whether the caller's highest role is **strictly above** `position`.
    ///
    /// Role management follows Discord: you cannot create or assign a role at or above
    /// your own highest — except the guild owner, who outranks everyone (ADR 0020).
    #[must_use]
    pub fn outranks_position(&self, position: i64) -> bool {
        self.actor_is_owner || self.actor_highest > position
    }

    /// Whether the caller outranks a member holding `target_roles` (ADR 0020).
    #[must_use]
    pub fn outranks_member(&self, target_roles: &[String]) -> bool {
        self.permissions
            .outranks(&self.actor_roles, target_roles, self.actor_is_owner)
    }

    /// The position of a role, if the guild defines it.
    #[must_use]
    pub fn position_of(&self, role_id: &str) -> Option<i64> {
        self.permissions.roles.iter().find(|r| r.id == role_id).map(|r| r.position)
    }

    /// The position to compare against for a channel-override target, treating
    /// `@everyone` as position 0.
    #[must_use]
    pub fn role_override_position(&self, role_id: &str) -> Option<i64> {
        if role_id == EVERYONE_ROLE_ID { Some(0) } else { self.position_of(role_id) }
    }

    /// The permissions a role carries (`@everyone`'s base for [`EVERYONE_ROLE_ID`]).
    #[must_use]
    pub fn role_permissions(&self, role_id: &str) -> Option<Permissions> {
        if role_id == EVERYONE_ROLE_ID {
            Some(self.permissions.everyone)
        } else {
            self.permissions.roles.iter().find(|r| r.id == role_id).map(|r| r.permissions)
        }
    }

    /// The caller's guild-level permissions, with `ADMINISTRATOR` expanded to all bits.
    #[must_use]
    pub fn guild_permissions(&self) -> Permissions {
        let base = self.permissions.base_permissions(&self.actor_roles);
        if base.contains(Permissions::ADMINISTRATOR) { Permissions::ALL } else { base }
    }

    /// Whether the caller may grant `allow`.
    ///
    /// Discord-aligned and load-bearing: **you cannot grant a permission you do not
    /// hold yourself**. Without this, `MANAGE_ROLES` at a high position would let a
    /// member mint a lower role (or override, or assignment) carrying powers they lack
    /// — a privilege-escalation path (ADR 0020's hierarchy alone does not close it).
    #[must_use]
    pub fn can_grant(&self, allow: Permissions) -> bool {
        self.guild_permissions().contains(allow)
    }
}

/// Whether a channel-override state key names a **user** (vs. a role id).
///
/// `@everyone` is the reserved role id; otherwise a value that parses as a Matrix user
/// id is a user target, and anything else is a role id.
#[must_use]
pub fn is_user_target(target: &str) -> bool {
    target != EVERYONE_ROLE_ID && ruma::UserId::parse(target).is_ok()
}

/// Convert stored override rows into the protocol's [`Override`]s for resolution
/// (ADR 0020).
#[must_use]
pub fn overrides_from_rows(rows: &[OverrideRow]) -> Vec<Override> {
    rows.iter()
        .map(|row| Override {
            target: if is_user_target(&row.target) {
                match ruma::UserId::parse(&row.target) {
                    Ok(user) => OverrideTarget::User(user),
                    Err(_) => OverrideTarget::Role(row.target.clone()),
                }
            } else {
                OverrideTarget::Role(row.target.clone())
            },
            allow: Permissions::from_bits(row.allow as u64),
            deny: Permissions::from_bits(row.deny as u64),
        })
        .collect()
}

/// Load a guild and resolve the caller's standing, or `None` if the guild is unknown.
pub async fn load(
    projection: &Projection,
    guild_id: &str,
    actor: &str,
) -> Result<Option<GuildAccess>> {
    let Some(guild) = projection.get_guild(guild_id).await? else {
        return Ok(None);
    };

    let mut everyone = Permissions::NONE;
    let mut roles = Vec::new();
    for row in projection.list_roles(guild_id).await? {
        let row_permissions = row.perms();
        if row.role_id == EVERYONE_ROLE_ID {
            everyone = row_permissions;
        } else {
            roles.push(Role {
                id: row.role_id,
                position: row.position,
                permissions: row_permissions,
            });
        }
    }
    let permissions = GuildPermissions { everyone, roles };

    let actor_roles = projection
        .get_member(guild_id, actor)
        .await?
        .map(|member| member.roles)
        .unwrap_or_default();
    let actor_highest = permissions.highest_position(&actor_roles);
    let actor_membership = projection
        .get_membership(guild_id, actor)
        .await?
        .map(|membership| membership.membership);
    let actor_is_owner = projection.get_guild_owner(guild_id).await?.as_deref() == Some(actor);

    Ok(Some(GuildAccess {
        guild,
        permissions,
        actor_roles,
        actor_highest,
        actor_membership,
        actor_is_owner,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn access(everyone: Permissions, roles: Vec<(&str, i64, Permissions)>, mine: &[&str]) -> GuildAccess {
        let permissions = GuildPermissions {
            everyone,
            roles: roles
                .into_iter()
                .map(|(id, position, perms)| Role { id: id.to_owned(), position, permissions: perms })
                .collect(),
        };
        let actor_roles: Vec<String> = mine.iter().map(|s| (*s).to_owned()).collect();
        let actor_highest = permissions.highest_position(&actor_roles);
        GuildAccess {
            guild: GuildRow {
                guild_id: "!g:example.org".to_owned(),
                home_server: "example.org".to_owned(),
                name: None,
                join_mode: None,
                directory_visibility: None,
                default_notification_level: None,
                event_id: "$g".to_owned(),
            },
            permissions,
            actor_roles,
            actor_highest,
            actor_membership: Some("join".to_owned()),
            actor_is_owner: false,
        }
    }

    #[test]
    fn the_owner_bypasses_the_hierarchy() {
        let mut a = access(Permissions::NONE, vec![("mod", 50, Permissions::MANAGE_ROLES)], &["mod"]);
        assert!(!a.outranks_position(60), "a moderator is below a higher role");
        a.actor_is_owner = true;
        assert!(a.is_owner());
        assert!(a.outranks_position(60), "the owner outranks everyone");
        assert!(a.outranks_member(&["vip".to_owned()]));
    }

    #[test]
    fn membership_requires_join() {
        let mut a = access(Permissions::NONE, vec![], &[]);
        assert!(a.is_member());
        a.actor_membership = Some("leave".to_owned());
        assert!(!a.is_member(), "a left/banned user is not a member");
        a.actor_membership = None;
        assert!(!a.is_member(), "missing membership is not a member");
    }

    #[test]
    fn admin_short_circuits_guild_permissions() {
        let a = access(Permissions::NONE, vec![("admin", 100, Permissions::ADMINISTRATOR)], &["admin"]);
        assert!(a.has(Permissions::MANAGE_ROLES));
        assert!(a.has(Permissions::BAN_MEMBERS));
    }

    #[test]
    fn hierarchy_is_strict() {
        let a = access(Permissions::NONE, vec![("mod", 50, Permissions::MANAGE_ROLES)], &["mod"]);
        assert!(a.outranks_position(49));
        assert!(!a.outranks_position(50), "a peer role is not below you");
        assert!(a.outranks_member(&[]));
        assert!(!a.outranks_member(&["mod".to_owned()]));
    }

    #[test]
    fn position_of_finds_defined_roles_only() {
        let a = access(Permissions::NONE, vec![("mod", 50, Permissions::NONE)], &["mod"]);
        assert_eq!(a.position_of("mod"), Some(50));
        assert_eq!(a.position_of("nope"), None);
    }
}
