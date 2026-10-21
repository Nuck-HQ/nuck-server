# ADR 0022 — Testing & CI strategy

- Status: Accepted
- Date: 2026-10-21

## Context

Federation means correctness depends on two servers agreeing, and the permission
engine has an invariant spanning two representations (the engine's computed
permissions and the mirrored `m.room.power_levels`).

## Decision

- **On every PR (fast):** unit tests + property tests + schema-registry golden tests.
- **Nightly:** the **two-server federation harness** and **Matrix Complement**.
- **Property test — the core invariant:** *effective permissions computed by the
  engine must equal the permissions implied by the mirrored `m.room.power_levels`
  (ADR 0006/0020).* Randomized, not case-based. This is the invariant the entire
  federation security model rests on, and it must exist from day one.
- **Unit tests:** permission resolution, override precedence, role hierarchy,
  bitfield versioning, and tolerant event parsing (unknown fields must never cause
  rejection).
- **Integration harness:** docker-compose a **two-server federation** (two full
  stacks: Tuwunel + nuck-server + Postgres each) and drive real cross-server flows —
  invite redemption, guild join, role-change propagation, mirrored PL update, bot
  gateway, voice token issuance.
- **Complement:** validates the substrate and that custom events do not break room
  state resolution.
- **Schema registry tests:** golden tests asserting every `space.nuck.*` type has a
  registered version and that new versions never mutate old ones (ADR 0014).

## Consequences

- The most expensive tests run nightly, keeping PR feedback fast; the security
  invariant still runs on every PR because it is cheap.
- Federation regressions are caught within a day, not at release.
- We maintain a multi-stack CI harness — real infrastructure, but it is the only way
  to test the product's defining property.
- Complement failures may be substrate (Tuwunel) issues; a triage policy is needed.

## Outcome (2026-10-21)

The property test was implemented ([nuck-protocol's `mirror_invariant` test](https://github.com/nuck-hq/nuck-protocol/blob/v0.2.0/tests/mirror_invariant.rs)) and it
**found a real flaw**: a scalar power level cannot mirror an arbitrary permission set.
The mirror never under-grants but over-grants for non-level-closed sets. See
**ADR 0040** for the finding, the options, and the pending decision. The test suite
currently documents the limit (exact for level-closed sets, permissive otherwise)
rather than asserting exactness.

**Revised by ADR 0041.** The fine-grained mirror is no longer the enforcement path, so
the exactness property test is the wrong target. The invariant tests should become:
(a) the **coarse tiering** (members vs. the action bot) is exact — members cannot
perform privileged writes; and (b) the **authorization engine** itself (role
resolution, overrides, hierarchy). The existing `mirror_invariant.rs` tests are kept
as documentation of why mirroring is not used.
