# ADR 0032 — Call E2EE: none in v1; DM-call E2EE deferred

- Status: Accepted
- Date: 2026-10-21

## Context

ADR 0008 placed guild channels as plaintext and DMs as E2EE. Call media was not
placed on either side. LiveKit supports insertable-streams E2EE, but at the cost of
key management, all-participant support, and the loss of server-side processing
(recording, transcription, moderation).

## Decision

- **v1: no call E2EE (option a).** Media is encrypted in transit (WebRTC DTLS-SRTP),
  but the SFU can access it — consistent with guild text channels being plaintext.
- **Target: (c) — DM calls E2EE, guild voice plaintext.** This mirrors ADR 0008 and
  mirrors Discord (DM calls E2EE, server voice not). Deferred.

## Consequences

- Voice is placed on the same line as text: guild voice is moderatable and
  SFU-accessible; DM calls will eventually be E2EE.
- **Until (c) lands, DM calls are not E2EE even though DM text is.** The client must
  not imply otherwise.
- If (c) is implemented, key distribution rides Matrix (per-room keys, ADR 0008
  mechanics), all participants must support it, and server-side processing for those
  calls is off the table.
- Full E2EE for all calls (option b) is rejected: it forfeits recording,
  transcription, and server-side moderation for guild voice, which is the primary use
  case.
