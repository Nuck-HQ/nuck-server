//! The core federation invariant (ADR 0022) and the limits of mirroring.
//!
//! The permissions the **engine** computes (ADR 0020) must agree with what the
//! **substrate** enforces through the mirrored `m.room.power_levels`. These tests
//! establish what is actually guaranteed — and, deliberately, what is not, which is
//! the finding recorded in ADR 0040.

use nuck_protocol::permissions::{
    Action, PERMISSION_TABLE, Permissions, engine_grants, substrate_grants,
};

/// Deterministic xorshift64* PRNG, so these property tests are reproducible without
/// a dependency.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        self.0 = x;
        x.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }
}

/// The permission set a member reaches at `level`: every permission whose
/// `required_level` is at or below it.
fn level_closed_set(level: i64) -> Permissions {
    PERMISSION_TABLE
        .iter()
        .filter(|d| d.required_level <= level)
        .fold(Permissions::NONE, |acc, d| acc.union(d.permission))
}

/// Whether a set is exactly the set implied by some level (a *level-closed* set).
///
/// `ADMINISTRATOR` is treated as closed: it grants everything, and so does the
/// substrate at level 100.
fn is_level_closed(p: Permissions) -> bool {
    if p.contains(Permissions::ADMINISTRATOR) {
        return true;
    }
    let max_level = PERMISSION_TABLE
        .iter()
        .filter(|d| p.contains(d.permission))
        .map(|d| d.required_level)
        .max()
        .unwrap_or(0);
    PERMISSION_TABLE
        .iter()
        .all(|d| p.contains(d.permission) == (d.required_level <= max_level))
}

/// **Guaranteed:** the substrate never *denies* something the engine grants.
/// Functionality is preserved — the mirror is permissive, never restrictive.
#[test]
fn substrate_never_undergrants() {
    let mut rng = Rng(0x9E37_79B9_7F4A_7C15);
    let mut samples: Vec<Permissions> = vec![Permissions::NONE];
    samples.extend(PERMISSION_TABLE.iter().map(|d| d.permission));
    for _ in 0..200_000 {
        samples.push(Permissions::from_bits(rng.next()));
    }

    for p in samples {
        for action in Action::ALL {
            assert!(
                !engine_grants(p, action) || substrate_grants(p, action),
                "substrate under-granted {action:?} for {:#018x}",
                p.bits()
            );
        }
    }
}

/// **Guaranteed:** for *level-closed* permission sets, the engine and the substrate
/// agree exactly, in both directions.
#[test]
fn exact_for_level_closed_sets() {
    for level in 0..=100 {
        let p = level_closed_set(level);
        assert!(is_level_closed(p), "level_closed_set({level}) is not closed");
        for action in Action::ALL {
            assert_eq!(
                engine_grants(p, action),
                substrate_grants(p, action),
                "divergence at level {level} for {action:?}"
            );
        }
    }
}

/// **Characterization of the limit (ADR 0040):** divergence — the substrate granting
/// what the engine denies — happens **only** for sets that are *not* level-closed.
/// Arbitrary permission combinations cannot be mirrored onto a scalar power level.
#[test]
fn divergence_implies_not_level_closed() {
    let mut rng = Rng(0xD1B5_4A32_D192_ED03);
    let mut divergences = 0u32;

    for _ in 0..200_000 {
        let p = Permissions::from_bits(rng.next());
        let diverges = Action::ALL
            .iter()
            .any(|&a| engine_grants(p, a) != substrate_grants(p, a));
        if diverges {
            divergences += 1;
            assert!(
                !is_level_closed(p),
                "divergence on a level-closed set {:#018x} — the guarantee is broken",
                p.bits()
            );
        }
    }

    assert!(
        divergences > 0,
        "expected some divergence for arbitrary sets; the characterization is stale"
    );
}

/// **ADR 0041:** the *coarse* tiering — the power levels Nuck actually writes — is
/// exact. A member can send messages and nothing else privileged; the action bot can
/// do everything. This is the invariant that replaces the fine-grained mirror.
#[test]
fn coarse_tiering_is_exact() {
    use nuck_protocol::permissions::{
        BOT_LEVEL, MEMBER_LEVEL, coarse_power_levels, level_allows,
    };

    let mirror = coarse_power_levels();
    for action in Action::ALL {
        assert!(
            level_allows(BOT_LEVEL, &mirror, action),
            "the action bot must be able to {action:?}"
        );
        assert_eq!(
            level_allows(MEMBER_LEVEL, &mirror, action),
            action == Action::SendMessage,
            "member tier is wrong for {action:?}: members may only send messages"
        );
    }
}
