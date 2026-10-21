# ADR 0042 — Protocol specification artifacts: permission table, event registry, role IDs

- Status: Accepted
- Date: 2026-10-21

## Context

ADR 0020 fixed the *approach* to permissions but explicitly left the concrete bit
assignments and the exact power-level mapping as an open question. ADR 0014 mandated a
`space.nuck.*` event registry but left it a **draft**: no concrete content schemas, no
role-ID scheme, and no owner for the registry mechanism. With `nuck-protocol` now its
own repository (ADR 0027), these artifacts are the crate's reason to exist and must be
finalized before client and federation work builds on them.

## Decision

1. **The permission table is finalized.** The canonical bit assignment and per-bit
   `level` ordering are defined in `nuck-protocol/src/permissions.rs` and documented in
   [`protocol/permissions.md`](../protocol/permissions.md). Four permissions that were
   declared in code but absent from the table (`PRIORITY_SPEAKER`, `STREAM`,
   `SEND_TTS_MESSAGES`, `USE_EXTERNAL_EMOJIS`) are added. Tests assert that **every**
   declared bit has a table entry and vice versa, that bits are unique and in range, and
   that Nuck extension bits live in the reserved `56..=63` range.
2. **`required_level` is demoted to ordering metadata** (per ADR 0041). It is not an
   enforcement threshold: the substrate mapping is the coarse tiering
   (`coarse_power_levels`, ADR 0041), and all fine-grained authorization is computed by
   the engine. The fine-grained `mirror()` is retained **only** as documentation of
   ADR 0040's finding and to drive the `mirror_invariant` tests.
3. **The event registry is owned by `nuck-protocol`.** `events::event_types` plus the
   `#[ruma_event]` content structs are canonical; `tests/registry.rs` enforces that every
   constant matches its content type's `StaticEventContent::TYPE`, that names are
   well-formed, versioned, and unique, and that parsing is tolerant (ADR 0014).
   `docs/protocol/events.md` is derived from the code.
4. **Missing content types are added:** `space.nuck.emoji.v1`,
   `space.nuck.sticker.v1`, and `space.nuck.interaction.v1` (ADR 0018/0023).
5. **Role-ID scheme:** the base role id is `everyone` (display name `@everyone`); every
   other role id is opaque — `r` + 12 lowercase hex characters, randomly generated.
   Role order is carried by `position` on the role event; ties break by id.
6. **Account data stays client-local** and is *not* part of the federated registry or
   the crate yet (its content types arrive with the features that need them).

## Consequences

- The bitfield, the coarse power-level mapping, and every `space.nuck.*` type are now
  concrete, versioned, and **test-enforced** — the client and federation can build
  against them (ADR 0026's compatibility window now has a real artifact to guard).
- Adding a bit or an event type is **additive** (same protocol version, tolerant
  parsing); repurposing a bit or mutating an existing type is **breaking** (new type /
  protocol-version bump, ADR 0014/0026).
- `required_level` remains vestigial; a future ADR may remove it once the legacy
  fine-grained mirror is dropped.
- The `/_nuck/v1` endpoint schemas, the federation feature-gate matrix, and the
  hosting/upgrade runbook remain open (see [open-questions](../open-questions.md)).
