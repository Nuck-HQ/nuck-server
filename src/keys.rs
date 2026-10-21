//! State-key encoding for user-id-keyed Nuck events.
//!
//! Tuwunel's room auth **rejects a state event whose `state_key` is another user's id**
//! even from the privileged action bot ("sender cannot send event with `state_key`
//! matching another user's ID"). Nuck's guild-level `member`, `ban`, and `join_request`
//! events — and per-user channel overrides — are keyed by user id, so the raw id cannot
//! be used as the state key.
//!
//! We therefore store the user id **without its leading `@`**. The result is no longer a
//! valid user id, so the auth check passes, and it is trivially reversible. The encoding
//! is a server/substrate detail: clients never see it — the v1 REST surface returns real
//! user ids.

/// Encode a user id for use as a state key (drop the leading `@`).
#[must_use]
pub fn encode_user_key(user_id: &str) -> String {
    user_id.strip_prefix('@').unwrap_or(user_id).to_owned()
}

/// Decode a user-id state key (restore the leading `@`). Tolerates an unencoded key.
#[must_use]
pub fn decode_user_key(key: &str) -> String {
    if key.starts_with('@') {
        key.to_owned()
    } else {
        format!("@{key}")
    }
}

/// Encode an override target: user ids lose their `@`; role ids are left unchanged.
#[must_use]
pub fn encode_override_target(target: &str) -> String {
    if target.starts_with('@') {
        encode_user_key(target)
    } else {
        target.to_owned()
    }
}

/// Decode an override target. A user id always contains `:` (localpart:server) and a
/// role id never does, so a key containing `:` is a user.
#[must_use]
pub fn decode_override_target(key: &str) -> String {
    if key.contains(':') {
        decode_user_key(key)
    } else {
        key.to_owned()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn user_keys_round_trip() {
        let id = "@bob:example.org";
        assert_eq!(encode_user_key(id), "bob:example.org");
        assert_eq!(decode_user_key(&encode_user_key(id)), id);
    }

    #[test]
    fn override_targets_round_trip_and_keep_role_ids() {
        assert_eq!(encode_override_target("@bob:example.org"), "bob:example.org");
        assert_eq!(decode_override_target("bob:example.org"), "@bob:example.org");
        assert_eq!(encode_override_target("r0123456789ab"), "r0123456789ab");
        assert_eq!(decode_override_target("r0123456789ab"), "r0123456789ab");
        assert_eq!(encode_override_target("everyone"), "everyone");
        assert_eq!(decode_override_target("everyone"), "everyone");
    }
}
