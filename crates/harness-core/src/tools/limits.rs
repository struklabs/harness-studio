//! The limits every surface (Playground, CLI, Control API, the core agent loop) shares. The
//! values live here once; clients read them through [`ToolLimits`] instead of keeping copies.

use std::time::Duration;

use serde::{Deserialize, Serialize};

use super::registry::DEFAULT_TIMEOUT;
use super::types::RESULT_MAX_CHARS;

/// Model -> tools -> model rounds per user message (the Playground's historic 8).
pub const MAX_ROUNDS: u32 = 8;

/// The limits as clients see them (`GET /tools`, the Playground's `tools_list`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct ToolLimits {
    /// Tool rounds an agent loop runs before it stops and says so.
    pub max_rounds: u32,
    /// Seconds one call may take (a tool may bound itself tighter).
    pub timeout_secs: u64,
    /// A result longer than this many characters is cut and the cut is marked.
    pub max_result_chars: usize,
}

impl Default for ToolLimits {
    fn default() -> Self {
        Self {
            max_rounds: MAX_ROUNDS,
            timeout_secs: DEFAULT_TIMEOUT.as_secs(),
            max_result_chars: RESULT_MAX_CHARS,
        }
    }
}

impl ToolLimits {
    pub fn timeout(&self) -> Duration {
        Duration::from_secs(self.timeout_secs)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_defaults_are_the_documented_limits() {
        let l = ToolLimits::default();
        assert_eq!(
            (l.max_rounds, l.timeout_secs, l.max_result_chars),
            (8, 120, 16_000)
        );
    }
}
