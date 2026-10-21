-- Guild notification level (ADR 0031): projected alongside the other
-- `space.nuck.guild.v1` fields so a settings PATCH can re-write the full event.
-- Nullable: rows predating this migration fall back to the default on write.
ALTER TABLE guilds ADD COLUMN default_notification_level TEXT;
