//! The reconciliation loop (ADR 0019).
//!
//! The AppService push is the pipeline's *input*, but push can be missed or
//! out-of-order, and the store may be wiped. Reconciliation makes the pipeline
//! **self-healing**: on startup and periodically it re-reads each room's resolved state
//! with the admin visibility bypass and rebuilds that room's projection, pruning rows
//! whose event is gone (see [`Projection::reconcile_room`]).
//!
//! It deliberately does **not** replay events to reconstruct *current state* — it
//! trusts the substrate's state resolution, which is what federates correctly
//! (ADR 0019). The single exception is message **backfill** on startup, which fetches
//! recent timeline events to rebuild the search index after a wipe.
//!
//! [`Projection::reconcile_room`]: super::Projection::reconcile_room

use std::time::Duration;

use anyhow::Result;

use nuck_protocol::events::event_types as et;

use super::{Projection, ProjectionOp, ingest};
use crate::matrix::{RawEvent, SubstrateClient};

/// The outcome of one reconciliation pass.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct ReconcileReport {
    /// Rooms whose state was fetched and applied.
    pub rooms: usize,
    /// Projection operations applied across those rooms.
    pub ops: usize,
    /// Messages indexed by the startup backfill.
    pub backfilled: usize,
    /// Rooms that could not be reconciled (logged, not fatal).
    pub failures: usize,
}

/// Map a room's full resolved state to projection operations and the set of current
/// state event ids.
///
/// Pure, so the mapping is unit-testable without a substrate or a database. The event
/// ids are *all* current state event ids (not just Nuck ones): every projected row
/// records the Nuck event that wrote it, so comparing against the full set still
/// detects removed rows, and never falsely prunes a live one.
pub fn state_ops(state: &[RawEvent]) -> (Vec<ProjectionOp>, Vec<String>, bool) {
    let mut ops = Vec::new();
    let mut event_ids = Vec::with_capacity(state.len());
    let mut is_guild = false;

    for event in state {
        event_ids.push(event.event_id.clone());
        if event.event_type == et::GUILD {
            is_guild = true;
        }
        match ingest::project(event) {
            Ok(Some(op)) => ops.push(op),
            Ok(None) => {}
            Err(e) => tracing::warn!(
                event_id = %event.event_id,
                event_type = %event.event_type,
                "skipping malformed state event: {e}"
            ),
        }
    }

    (ops, event_ids, is_guild)
}

/// Drives the periodic reconciliation pass (ADR 0019).
#[derive(Clone)]
pub struct Reconciler {
    projection: Projection,
    substrate: SubstrateClient,
    /// `(AppService token, projector user)` — the identity that reads room state and
    /// joins guilds. The projector is a normal member of every guild room, so no admin
    /// privilege is needed (ADR 0046).
    projector: Option<(String, String)>,
    interval: Duration,
    /// Messages fetched per channel during a backfill; `None` disables backfill.
    backfill_limit: Option<u32>,
}

impl Reconciler {
    /// Build a reconciler. `projector` is the `(AppService token, projector user)` pair;
    /// `backfill_limit` bounds the per-channel message backfill.
    #[must_use]
    pub fn new(
        projection: Projection,
        substrate: SubstrateClient,
        projector: Option<(String, String)>,
        interval: Duration,
        backfill_limit: Option<u32>,
    ) -> Self {
        Self { projection, substrate, projector, interval, backfill_limit }
    }

    /// Run one reconciliation pass: every known guild, then every known channel.
    ///
    /// Guilds are reconciled first so newly created channels become known within the
    /// same pass. `backfill` indexes recent channel messages (startup only).
    pub async fn reconcile_once(&self, backfill: bool) -> Result<ReconcileReport> {
        let mut report = ReconcileReport::default();

        let mut guilds = self.projection.guild_ids().await?;
        if let Some((token, user)) = &self.projector {
            match self.substrate.joined_rooms(token, Some(user)).await {
                Ok(discovered) => {
                    for room in discovered {
                        if !guilds.contains(&room) {
                            guilds.push(room);
                        }
                    }
                }
                Err(e) => tracing::warn!(
                    "room discovery failed; reconciling only rooms already projected: {e}"
                ),
            }
        }

        for room in &guilds {
            self.reconcile_into(&mut report, room, false).await;
        }

        for room in self.projection.channel_ids().await? {
            if guilds.contains(&room) {
                continue;
            }
            self.reconcile_into(&mut report, &room, backfill).await;
        }

        Ok(report)
    }

    /// Reconcile one room, folding the outcome (or failure) into `report`.
    async fn reconcile_into(&self, report: &mut ReconcileReport, room_id: &str, backfill: bool) {
        match self.reconcile_room(room_id, backfill).await {
            Ok((ops, backfilled)) => {
                report.rooms += 1;
                report.ops += ops;
                report.backfilled += backfilled;
            }
            Err(e) => {
                report.failures += 1;
                tracing::warn!(room = %room_id, "reconcile failed: {e}");
            }
        }
    }

    /// Reconcile a single room: apply resolved state, prune drift, keep the projector
    /// joined, and (optionally) backfill messages. Returns `(ops applied, messages
    /// backfilled)`.
    pub async fn reconcile_room(&self, room_id: &str, backfill: bool) -> Result<(usize, usize)> {
        let Some((token, user)) = &self.projector else {
            anyhow::bail!("no projector identity configured; cannot read room state");
        };
        let state = self.substrate.room_state_as(token, Some(user), room_id).await?;
        let (ops, event_ids, is_guild) = state_ops(&state);

        let applied = self.projection.reconcile_room(room_id, &ops, &event_ids).await?;

        // ADR 0019: the projector must be in every guild room to receive federated
        // events. Joining is best-effort — a failure is logged, never fatal.
        if is_guild {
            if let Err(e) = self.substrate.join_room(token, Some(user), room_id).await {
                tracing::warn!(room = %room_id, user = %user, "projector auto-join failed: {e}");
            }
        }

        let mut backfilled = 0;
        if backfill {
            if let Some(limit) = self.backfill_limit {
                backfilled = self.backfill_messages(room_id, limit).await;
            }
        }

        self.projection.set_state(&reconcile_key(room_id), "ok").await.ok();
        Ok((applied, backfilled))
    }

    /// Index the most recent timeline events of a channel room (ADR 0029).
    async fn backfill_messages(&self, room_id: &str, limit: u32) -> usize {
        let Some((token, user)) = &self.projector else {
            return 0;
        };
        let messages = match self.substrate.room_messages_as(token, Some(user), room_id, limit).await
        {
            Ok(messages) => messages,
            Err(e) => {
                tracing::warn!(room = %room_id, "message backfill failed: {e}");
                return 0;
            }
        };

        let mut indexed = 0;
        for event in &messages {
            match ingest::project(event) {
                Ok(Some(op)) => {
                    if let Err(e) = self.projection.apply(op).await {
                        tracing::warn!(event_id = %event.event_id, "backfill apply failed: {e}");
                    } else {
                        indexed += 1;
                    }
                }
                Ok(None) => {}
                Err(e) => tracing::warn!(event_id = %event.event_id, "skipping malformed event: {e}"),
            }
        }
        indexed
    }

    /// Loop forever, reconciling at the configured cadence. The startup pass is run by
    /// the caller so its failures are visible before serving.
    pub async fn run(self) {
        let mut ticker = tokio::time::interval(self.interval);
        ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        ticker.tick().await; // consume the immediate first tick

        loop {
            ticker.tick().await;
            match self.reconcile_once(false).await {
                Ok(report) => tracing::info!(
                    rooms = report.rooms,
                    ops = report.ops,
                    failures = report.failures,
                    "reconciliation pass complete"
                ),
                Err(e) => tracing::warn!("reconciliation pass failed: {e}"),
            }
        }
    }
}

fn reconcile_key(room_id: &str) -> String {
    format!("reconcile:{room_id}")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn event(event_type: &str, state_key: Option<&str>, event_id: &str) -> RawEvent {
        RawEvent {
            event_id: event_id.to_owned(),
            room_id: "!guild:example.org".to_owned(),
            sender: "@owner:example.org".to_owned(),
            event_type: event_type.to_owned(),
            state_key: state_key.map(ToOwned::to_owned),
            origin_server_ts: Some(1_700_000_000_000),
            content: serde_json::json!({}),
        }
    }

    #[test]
    fn state_ops_collects_ids_and_detects_guilds() {
        let state = vec![
            event(et::GUILD, Some(""), "$guild"),
            event("m.room.member", Some("@u:example.org"), "$member"),
        ];
        let (ops, ids, is_guild) = state_ops(&state);
        // The guild event has empty content, which now encodes removal (there is no
        // state deletion); the member event is not projected. Both ids are recorded.
        assert_eq!(ops.len(), 1);
        assert!(matches!(ops[0], ProjectionOp::RemoveGuild(ref id) if id == "!guild:example.org"));
        assert_eq!(ids, vec!["$guild".to_owned(), "$member".to_owned()]);
        assert!(is_guild, "the guild event marks the room as a guild");
    }

    #[test]
    fn state_ops_ignores_non_guild_rooms() {
        let state = vec![event("m.room.member", Some("@u:example.org"), "$m")];
        let (ops, ids, is_guild) = state_ops(&state);
        assert!(ops.is_empty());
        assert_eq!(ids.len(), 1);
        assert!(!is_guild);
    }
}
