//! Cursor pagination (ADR 0013, `docs/protocol/api.md`).
//!
//! Uniform across every list endpoint: `?limit=<1..200>&cursor=<opaque>`, and the
//! response carries an optional `next_cursor`. The cursor value is opaque to the
//! client; the server treats it as the raw ordering key of the last returned item.

use serde::Deserialize;

/// The `limit`/`cursor` query parameters shared by every list endpoint.
#[derive(Debug, Clone, Deserialize)]
pub struct PageQuery {
    #[serde(default)]
    pub limit: Option<u32>,
    #[serde(default)]
    pub cursor: Option<String>,
}

impl PageQuery {
    /// The page size, clamped to `1..=200` (default 50).
    #[must_use]
    pub fn limit(&self) -> i64 {
        i64::from(self.limit.unwrap_or(50).clamp(1, 200))
    }

    /// The cursor, or `None` for the first page (an empty string is no cursor).
    #[must_use]
    pub fn cursor(&self) -> Option<&str> {
        self.cursor.as_deref().filter(|cursor| !cursor.is_empty())
    }

    /// Whether `len` rows filled the page (so a `next_cursor` should be returned).
    #[must_use]
    pub fn is_full(&self, len: usize) -> bool {
        len as i64 == self.limit()
    }
}
