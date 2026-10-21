//! The permission engine: effective-permission computation and power-level
//! mirroring (ADR 0006/0020).
//!
//! Per **ADR 0041**, the substrate's power levels are used only **coarsely**
//! (members vs. the action bot); the authoritative enforcement is this engine,
//! evaluated in nuck-server. See [`nuck_protocol::authorization`] for the resolution
//! algorithm, [`crate::authorization`] for loading it from projected guild state, and
//! [`crate::bot`] for the privileged execution path.

pub use nuck_protocol::authorization::{
    EVERYONE_ROLE_ID, GuildPermissions, Override, OverrideTarget, Role,
};
pub use nuck_protocol::permissions::{
    Action, BOT_LEVEL, MEMBER_LEVEL, PERMISSION_TABLE, Permissions, PowerLevelsMirror,
    coarse_power_levels, effective_level, engine_grants, mirror, substrate_grants,
};
