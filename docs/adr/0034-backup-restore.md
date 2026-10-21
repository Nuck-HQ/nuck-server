# ADR 0034 — Backup & restore: one-command consistent snapshot

- Status: Accepted
- Date: 2026-10-21

## Context

A self-hosted community server depends on reliable backups. Tuwunel's storage is
RocksDB, which cannot be safely backed up by copying a live directory.

## Decision

**One-command consistent snapshot (option a).**

- **`nuck backup`** produces a coordinated, restorable snapshot of Tuwunel
  (DB + media) and Postgres; **`nuck restore`** applies it.
- **RocksDB is handled explicitly:** either a **RocksDB checkpoint** (if Tuwunel
  exposes one via admin API — to verify) or a **brief coordinated quiesce** (pause
  writes, snapshot, resume). A live filesystem copy is unsafe and is not the default.
- **Media** is filesystem-based and is snapshotted alongside the DB.
- **Postgres is included but optional to restore** — it is a derived index
  (ADR 0003) and rebuildable, but restoring it avoids a long replay.
- **`nuck restore` is tested in CI** against the two-server harness (ADR 0022); an
  untested restore is not a backup.
- **A version manifest** recording the exact version matrix (ADR 0026) is part of
  every snapshot; a restore refuses to apply a snapshot to an incompatible version
  rather than silently corrupting state.
- **Continuous replication / PITR (c) is deferred.**

## Consequences

- Operators get a single, trustworthy backup/restore path.
- Backups are version-aware, preventing the most insidious failure mode
  (restoring into a mismatched substrate).
- CI must exercise a full backup → wipe → restore cycle, which is non-trivial to
  automate but is the only way to trust it.
- Backup/restore orchestration must coordinate across containers (Tuwunel, Postgres,
  media volume), making the CLI a real component, not a shell script.

## Verification update (2026-10-21)

Checked against Tuwunel
([verification](../verification/tuwunel.md#1-backup--checkpoint--confirmed-better-than-assumed)):
Tuwunel provides **online** managed backups (`server backup-database`,
`database_backup_path` + `database_backups_to_keep`, `verify-backup`) and **online
checkpoints** (`server checkpoint-database [--path …]`), plus `tuwunel --restore-backup`
for restore.

**Resolved:** no coordinated quiesce is needed — the Decision's "checkpoint *or*
quiesce" is settled in favour of the **online checkpoint**. Caveats: these are
**admin-room commands, not HTTP endpoints**, so `nuck backup` invokes them via
`tuwunel --maintenance --execute "server checkpoint-database --path …"`; media and any
storage providers are still snapshotted separately; and the managed backup repository
is **not** itself a usable database directory (checkpoints are).

