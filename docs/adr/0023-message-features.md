# ADR 0023 — Message features: inherit Matrix; custom only for Discord-specific

- Status: Accepted
- Date: 2026-10-21

## Context

A chat client needs replies, reactions, edits, redactions, attachments, and threads.
Matrix defines all of these, and `matrix-rust-sdk` already implements their client
handling. Discord adds a few things Matrix has no equivalent for.

## Decision

**Inherit Matrix for standard messaging (option a).**

- **Matrix primitives:** replies (`m.in_reply_to`), reactions (`m.reaction`), edits
  (`m.replace`), redactions, attachments (`m.image`/`m.file`/`m.video`/`m.audio`),
  and threads (MSC3440 `m.thread`).
- **Custom `space.nuck.*` only for Discord-specific:** rich embeds, slash-command
  invocations, and stickers — delivered/created via the bot gateway (ADR 0018).

## Consequences

- The client gets rendering, pagination, and state handling for standard messages
  from `matrix-rust-sdk`; these features federate with no new work.
- **Threads follow Matrix's thread relation, not Discord's per-message threads** —
  close, but not identical. Accepted.
- **Embeds are a custom render path**; the client handles both plain Matrix messages
  and `space.nuck.embed` events.
- **Message search** (ADR 0019) must understand both Matrix message events and Nuck
  embed/slash events to index them meaningfully, and covers plaintext guild channels
  only (ADR 0008).
