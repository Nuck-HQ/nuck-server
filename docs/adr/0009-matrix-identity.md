# ADR 0009 — Matrix user IDs are canonical Nuck identity

- Status: Accepted
- Date: 2026-10-21

## Context

Nuck is a federated network of self-hosted servers (ADR 0001). Identity must be
globally unique and cross-server addressable. Options were Nuck-owned accounts
mapped onto internal Matrix users, or reusing Matrix identity directly.

## Decision

**Matrix user IDs are canonical Nuck identity (option a).**

- A user is `@user:server`; the server part is their **home Nuck server**.
- **Registration** is per-server, open or invite-gated, configurable per server.
- **Auth** passes through to Tuwunel's login; the client presents the resulting
  Matrix **access token** to nuck-server on every call.
- **Devices and E2EE keys** are Matrix's, so E2EE DMs (ADR 0008) work without a
  parallel identity system.
- **Nuck profile** (avatar, banner, bio, custom status) extends the Matrix profile
  via custom account data.
- **OIDC is a future login method** into this same model, not a replacement for it.

## Consequences

- Federated identity is free; remote users are addressable with no mapping layer.
- We inherit Tuwunel's registration, session, device, and key infrastructure.
- Account lifecycle (deactivation, GDPR export, session revocation) is bound to the
  substrate's semantics.
- A user's identity is permanently tied to their home server's `server_name`; server
  migration/renaming is not supported (consistent with Matrix).
