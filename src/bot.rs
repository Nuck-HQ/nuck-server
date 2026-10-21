//! The privileged **action bot** (ADR 0041).
//!
//! Under ADR 0041 the substrate's power levels are used only **coarsely**: ordinary
//! members can send messages, and a single AppService user — the action bot — sits at
//! the top of the ladder as the only writer that passes the `state_default`, `ban`,
//! `kick`, and `redact` thresholds. Every privileged guild write therefore goes out as
//! this user.
//!
//! The client's request is **not** a public room event; only the resulting action is.
//! Attribution is preserved separately: the action carries the acting user where the
//! event schema has a field for it, and the audit log always records the actor.

use anyhow::Result;

use crate::matrix::SubstrateClient;

/// A handle that performs substrate writes as the action bot.
pub struct ActionBot<'a> {
    substrate: &'a SubstrateClient,
    user: &'a str,
    token: &'a str,
}

impl<'a> ActionBot<'a> {
    /// Build a bot handle from a substrate client, the bot's user id, and its
    /// AppService token (ADR 0028).
    #[must_use]
    pub fn new(substrate: &'a SubstrateClient, user: &'a str, token: &'a str) -> Self {
        Self { substrate, user, token }
    }

    /// The bot's user id.
    #[must_use]
    pub fn user(&self) -> &str {
        self.user
    }

    /// Write a `space.nuck.*` state event into a guild room, returning its event id.
    pub async fn write_state(
        &self,
        room_id: &str,
        event_type: &str,
        state_key: &str,
        content: serde_json::Value,
    ) -> Result<String> {
        self.substrate
            .send_state_event(self.token, Some(self.user), room_id, event_type, state_key, content)
            .await
    }

    /// Ban a user from a guild room (a membership write, not a state event).
    pub async fn ban(&self, room_id: &str, target: &str, reason: Option<&str>) -> Result<()> {
        self.substrate
            .set_membership(self.token, Some(self.user), room_id, "ban", target, reason)
            .await
    }

    /// Kick a user from a guild room (rejoinable).
    pub async fn kick(&self, room_id: &str, target: &str, reason: Option<&str>) -> Result<()> {
        self.substrate
            .set_membership(self.token, Some(self.user), room_id, "kick", target, reason)
            .await
    }

    /// Unban a user from a guild room.
    pub async fn unban(&self, room_id: &str, target: &str) -> Result<()> {
        self.substrate
            .set_membership(self.token, Some(self.user), room_id, "unban", target, None)
            .await
    }
}
