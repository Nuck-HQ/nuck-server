//! The authorization engine (ADR 0020, ADR 0041).
//!
//! Resolves a member's effective permissions from the guild's roles and a channel's
//! overrides, and answers hierarchy questions for moderation.
//!
//! This is **protocol semantics**: every Nuck server must compute the same answer, and
//! it is the **authoritative** enforcement — the substrate's power levels are used only
//! coarsely (ADR 0041), so correctness here is what actually protects a guild.

use ruma::{OwnedUserId, UserId};
use serde::{Deserialize, Serialize};

use crate::permissions::Permissions;

/// The synthetic `everyone` role id: the base every member holds.
///
/// Deliberately **not** `@everyone`: Matrix treats a state key beginning with `@` as a
/// user id, and the substrate rejects writing such a role event. `@everyone` remains
/// the role's *display name*; this is its stable id.
pub const EVERYONE_ROLE_ID: &str = "everyone";

/// A guild role (ADR 0006).
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Role {
    /// Stable role id (the state key of `space.nuck.role.v1`).
    pub id: String,
    /// Higher position wins on conflict.
    pub position: i64,
    /// The role's permission set.
    pub permissions: Permissions,
}

/// The guild's role configuration: the `@everyone` base plus the defined roles.
#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct GuildPermissions {
    /// Base permissions every member holds.
    pub everyone: Permissions,
    /// The defined roles (excluding `@everyone`).
    pub roles: Vec<Role>,
}

/// A per-channel permission override (ADR 0020).
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Override {
    /// Who this override applies to.
    pub target: OverrideTarget,
    /// Bits this override grants.
    pub allow: Permissions,
    /// Bits this override removes.
    pub deny: Permissions,
}

/// The target of a channel override.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum OverrideTarget {
    /// A role id, or [`EVERYONE_ROLE_ID`].
    Role(String),
    /// A specific user.
    User(OwnedUserId),
}

impl GuildPermissions {
    fn role(&self, id: &str) -> Option<&Role> {
        self.roles.iter().find(|r| r.id == id)
    }

    /// The highest position among a member's roles (`@everyone` is position 0).
    #[must_use]
    pub fn highest_position(&self, member_roles: &[String]) -> i64 {
        member_roles
            .iter()
            .filter(|id| id.as_str() != EVERYONE_ROLE_ID)
            .filter_map(|id| self.role(id))
            .map(|r| r.position)
            .max()
            .unwrap_or(0)
    }

    /// Base permissions: `@everyone` ∪ the member's roles (ADR 0020, step 1).
    #[must_use]
    pub fn base_permissions(&self, member_roles: &[String]) -> Permissions {
        member_roles
            .iter()
            .filter(|id| id.as_str() != EVERYONE_ROLE_ID)
            .filter_map(|id| self.role(id))
            .fold(self.everyone, |acc, r| acc.union(r.permissions))
    }

    /// Effective permissions in a channel (ADR 0020's resolution algorithm).
    ///
    /// `ADMINISTRATOR` and the guild owner short-circuit to [`Permissions::ALL`] and
    /// ignore overrides. Overrides are applied in a fixed order — `@everyone`, then the
    /// member's roles by ascending position (ties by id), then the member — so every
    /// server computes the same result.
    #[must_use]
    pub fn channel_permissions(
        &self,
        member_roles: &[String],
        user_id: &UserId,
        actor_is_owner: bool,
        overrides: &[Override],
    ) -> Permissions {
        let base = self.base_permissions(member_roles);
        if actor_is_owner || base.contains(Permissions::ADMINISTRATOR) {
            return Permissions::ALL;
        }

        let mut p = base;

        // 1. The @everyone override.
        if let Some(o) = overrides
            .iter()
            .find(|o| matches!(&o.target, OverrideTarget::Role(r) if r == EVERYONE_ROLE_ID))
        {
            p = apply(p, o);
        }

        // 2. Role overrides, ordered by position ascending then id (deterministic).
        let mut role_overrides: Vec<(&Role, &Override)> = overrides
            .iter()
            .filter_map(|o| match &o.target {
                OverrideTarget::Role(r) if r != EVERYONE_ROLE_ID => self
                    .role(r)
                    .filter(|_| member_roles.iter().any(|m| m == r))
                    .map(|role| (role, o)),
                _ => None,
            })
            .collect();
        role_overrides.sort_by(|(ra, _), (rb, _)| {
            ra.position.cmp(&rb.position).then_with(|| ra.id.cmp(&rb.id))
        });
        for (_, o) in role_overrides {
            p = apply(p, o);
        }

        // 3. The member-specific override, last.
        if let Some(o) = overrides.iter().find(|o| match &o.target {
            OverrideTarget::User(u) => u.as_str() == user_id.as_str(),
            _ => false,
        }) {
            p = apply(p, o);
        }

        p
    }

    /// Whether a member holds `permission` at the guild level.
    #[must_use]
    pub fn has_permission(&self, member_roles: &[String], permission: Permissions) -> bool {
        let base = self.base_permissions(member_roles);
        base.contains(Permissions::ADMINISTRATOR) || base.contains(permission)
    }

    /// Whether `actor` outranks `target` for moderation (ADR 0006/0020).
    ///
    /// The guild owner outranks everyone; otherwise the actor's highest role must be
    /// **strictly** above the target's.
    #[must_use]
    pub fn outranks(
        &self,
        actor_roles: &[String],
        target_roles: &[String],
        actor_is_owner: bool,
    ) -> bool {
        actor_is_owner || self.highest_position(actor_roles) > self.highest_position(target_roles)
    }
}

/// Apply one override: deny wins within the override, allow adds.
fn apply(p: Permissions, o: &Override) -> Permissions {
    let allow = o.allow.difference(o.deny);
    p.difference(o.deny).union(allow)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn role(id: &str, position: i64, permissions: Permissions) -> Role {
        Role { id: id.to_owned(), position, permissions }
    }

    fn guild() -> GuildPermissions {
        GuildPermissions {
            everyone: Permissions::SEND_MESSAGES.union(Permissions::VIEW_CHANNEL),
            roles: vec![
                role("mod", 50, Permissions::MANAGE_MESSAGES.union(Permissions::KICK_MEMBERS)),
                role("admin", 100, Permissions::ADMINISTRATOR),
            ],
        }
    }

    fn user() -> &'static UserId {
        ruma::user_id!("@u:example.org")
    }

    #[test]
    fn base_is_everyone_union_roles() {
        let p = guild().base_permissions(&["mod".to_owned()]);
        assert!(p.contains(Permissions::SEND_MESSAGES));
        assert!(p.contains(Permissions::KICK_MEMBERS));
        assert!(!p.contains(Permissions::BAN_MEMBERS));
    }

    #[test]
    fn administrator_short_circuits_and_ignores_overrides() {
        let overrides = vec![Override {
            target: OverrideTarget::Role("admin".to_owned()),
            allow: Permissions::NONE,
            deny: Permissions::ALL,
        }];
        let p = guild().channel_permissions(&["admin".to_owned()], user(), false, &overrides);
        assert_eq!(p, Permissions::ALL);
    }

    #[test]
    fn the_owner_bypasses_overrides() {
        let overrides = vec![Override {
            target: OverrideTarget::Role(EVERYONE_ROLE_ID.to_owned()),
            allow: Permissions::NONE,
            deny: Permissions::ALL,
        }];
        // A plain member with no roles gets nothing; the owner gets everything.
        let p = guild().channel_permissions(&[], user(), false, &overrides);
        assert!(!p.contains(Permissions::SEND_MESSAGES));
        let p = guild().channel_permissions(&[], user(), true, &overrides);
        assert_eq!(p, Permissions::ALL);
    }

    #[test]
    fn deny_wins_within_an_override() {
        let overrides = vec![Override {
            target: OverrideTarget::Role(EVERYONE_ROLE_ID.to_owned()),
            allow: Permissions::SEND_MESSAGES,
            deny: Permissions::SEND_MESSAGES,
        }];
        let p = guild().channel_permissions(&[], user(), false, &overrides);
        assert!(!p.contains(Permissions::SEND_MESSAGES));
    }

    #[test]
    fn member_override_beats_role_override() {
        let overrides = vec![
            Override {
                target: OverrideTarget::Role("mod".to_owned()),
                allow: Permissions::NONE,
                deny: Permissions::MANAGE_MESSAGES,
            },
            Override {
                target: OverrideTarget::User(user().to_owned()),
                allow: Permissions::MANAGE_MESSAGES,
                deny: Permissions::NONE,
            },
        ];
        let p = guild().channel_permissions(&["mod".to_owned()], user(), false, &overrides);
        assert!(p.contains(Permissions::MANAGE_MESSAGES));
    }

    #[test]
    fn higher_role_override_applied_later() {
        let g = GuildPermissions {
            everyone: Permissions::NONE,
            roles: vec![role("low", 10, Permissions::NONE), role("high", 20, Permissions::NONE)],
        };
        let overrides = vec![
            Override {
                target: OverrideTarget::Role("high".to_owned()),
                allow: Permissions::NONE,
                deny: Permissions::SEND_MESSAGES,
            },
            Override {
                target: OverrideTarget::Role("low".to_owned()),
                allow: Permissions::SEND_MESSAGES,
                deny: Permissions::NONE,
            },
        ];
        // `low` is applied first (allow), `high` later (deny) → denied.
        let p = g.channel_permissions(&["low".to_owned(), "high".to_owned()], user(), false, &overrides);
        assert!(!p.contains(Permissions::SEND_MESSAGES));
    }

    #[test]
    fn hierarchy_requires_strictly_higher_position() {
        let g = guild();
        assert!(g.outranks(&["admin".to_owned()], &["mod".to_owned()], false));
        assert!(!g.outranks(&["mod".to_owned()], &["mod".to_owned()], false));
        assert!(g.outranks(&[], &["admin".to_owned()], true));
    }
}
