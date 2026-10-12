//! The tool-calling level of a model, learned from the gateway log, and how many tools it
//! should be given.
//!
//! The level is not a guess from the model's name: it is the success ratio of real tool rounds.
//! A round counts when the request offered tools and the model either made a clean call
//! (success) or failed in a way that is the model's or its template's doing (`runtime` /
//! `model` findings, see [`super::classify`]). Rounds where the model simply answered in text,
//! where the context was too small, or where the client's tool failed say nothing about the
//! model and are not counted.

use serde::{Deserialize, Serialize};

/// Rounds needed before a level is stated.
pub const MIN_ROUNDS: u64 = 5;
/// Success ratio from which the model's tool calls count as native.
pub const NATIVE_RATIO: f64 = 0.85;
/// Success ratio below which the model is limited (it fails about every other round).
pub const LIMITED_RATIO: f64 = 0.5;

/// How well a model handles tool calls.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ToolLevel {
    /// Calls arrive as structured `tool_calls` from the model's own chat template, reliably.
    Native,
    /// Calls work but not reliably (about one in five fails), or they work only through
    /// llama.cpp's generic tool format because the chat template has no tool handling.
    Generic,
    /// Fails about every other round or worse.
    Limited,
}

/// Measured tool rounds of one model.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ToolRounds {
    /// Rounds with a clean call.
    pub ok: u64,
    /// Rounds that failed through the model or its template.
    pub bad: u64,
}

impl ToolRounds {
    pub fn total(&self) -> u64 {
        self.ok + self.bad
    }

    /// `ok / (ok + bad)`; `None` without rounds.
    pub fn ratio(&self) -> Option<f64> {
        (self.total() > 0).then(|| self.ok as f64 / self.total() as f64)
    }

    /// The level, once [`MIN_ROUNDS`] are in. `template_tools` is what the chat template says
    /// (`Some(false)`: no tool handling): a model that succeeds anyway does so through the
    /// generic format, so it is not "native".
    pub fn level(&self, template_tools: Option<bool>) -> Option<ToolLevel> {
        if self.total() < MIN_ROUNDS {
            return None;
        }
        let r = self.ratio()?;
        Some(if r >= NATIVE_RATIO {
            if template_tools == Some(false) {
                ToolLevel::Generic
            } else {
                ToolLevel::Native
            }
        } else if r >= LIMITED_RATIO {
            ToolLevel::Generic
        } else {
            ToolLevel::Limited
        })
    }
}

/// The most tools worth enabling at once for a model of `params` parameters.
///
/// Every enabled tool costs context (about 150-250 tokens for its definition) and widens the
/// choice the model has to get right; small models lose accuracy quickly with a long list.
/// The size steps are rules of thumb that agent tools bear out (OpenCode and Claude Code send
/// 10-30 tools, which small models do not handle well); a measured weak level lowers the cap.
///
/// | parameters | tools |
/// |---|---|
/// | under 2 B | 4 |
/// | 2 - 5 B | 8 |
/// | 5 - 10 B | 12 |
/// | 10 - 20 B | 20 |
/// | 20 B and more | 32 |
/// | unknown | 8 |
///
/// A `Generic` level caps this at 8 and a `Limited` one at 4.
pub fn recommended_tools(params: Option<u64>, level: Option<ToolLevel>) -> u32 {
    let by_size = match params {
        None => 8,
        Some(p) if p < 2_000_000_000 => 4,
        Some(p) if p < 5_000_000_000 => 8,
        Some(p) if p < 10_000_000_000 => 12,
        Some(p) if p < 20_000_000_000 => 20,
        Some(_) => 32,
    };
    match level {
        Some(ToolLevel::Limited) => by_size.min(4),
        Some(ToolLevel::Generic) => by_size.min(8),
        _ => by_size,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rounds(ok: u64, bad: u64) -> ToolRounds {
        ToolRounds { ok, bad }
    }

    #[test]
    fn no_level_before_enough_rounds() {
        assert_eq!(rounds(0, 0).level(None), None);
        assert_eq!(rounds(4, 0).level(None), None);
        assert_eq!(rounds(0, 4).level(None), None);
        assert_eq!(rounds(5, 0).level(None), Some(ToolLevel::Native));
    }

    #[test]
    fn the_ratio_decides() {
        assert_eq!(rounds(17, 3).level(None), Some(ToolLevel::Native)); // 0.85
        assert_eq!(rounds(16, 4).level(None), Some(ToolLevel::Generic)); // 0.80
        assert_eq!(rounds(5, 5).level(None), Some(ToolLevel::Generic)); // 0.50
        assert_eq!(rounds(4, 6).level(None), Some(ToolLevel::Limited)); // 0.40
        assert_eq!(rounds(0, 12).level(None), Some(ToolLevel::Limited));
        assert_eq!(rounds(4, 1).ratio(), Some(0.8));
        assert_eq!(rounds(0, 0).ratio(), None);
    }

    #[test]
    fn a_model_without_template_tools_is_never_native() {
        assert_eq!(rounds(10, 0).level(Some(false)), Some(ToolLevel::Generic));
        assert_eq!(rounds(10, 0).level(Some(true)), Some(ToolLevel::Native));
        // Failing stays failing whatever the template says.
        assert_eq!(rounds(1, 9).level(Some(false)), Some(ToolLevel::Limited));
    }

    #[test]
    fn recommended_tools_by_size_and_level() {
        let b = |n: u64| Some(n * 1_000_000_000);
        assert_eq!(recommended_tools(b(1), None), 4);
        assert_eq!(recommended_tools(b(4), None), 8);
        assert_eq!(recommended_tools(b(8), None), 12);
        assert_eq!(recommended_tools(b(14), None), 20);
        assert_eq!(recommended_tools(b(70), None), 32);
        assert_eq!(recommended_tools(None, None), 8);
        // A weak measured level lowers the cap, a strong one leaves it.
        assert_eq!(recommended_tools(b(70), Some(ToolLevel::Generic)), 8);
        assert_eq!(recommended_tools(b(14), Some(ToolLevel::Limited)), 4);
        assert_eq!(recommended_tools(b(1), Some(ToolLevel::Limited)), 4);
        assert_eq!(recommended_tools(b(14), Some(ToolLevel::Native)), 20);
    }
}
