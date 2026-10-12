//! Summarization prompts. The instructions are English (models follow them best); the
//! summary itself is written in the conversation's language.

use super::Strategy;
use serde_json::{json, Value};

const STRUCTURED_SYSTEM: &str = "You compact chat conversations. Your summary REPLACES the \
transcript: afterwards the assistant sees only your summary and the newest messages, so \
anything you leave out is forgotten.";

const STRUCTURED_RULES: &str = "Rules:
- Write in the language the user writes in.
- Keep every concrete detail that may matter later and copy it exactly: names, numbers, \
amounts, dates, times, IDs and codes, file names, paths, URLs, settings, quotes.
- Record what the user wants (goals, preferences, constraints), what was answered or \
decided, and what is still open.
- For tool calls keep only which tool ran and the result details that matter.
- Do not invent or add anything; no commentary, no preamble.
Use these headings (leave out empty ones):
## Goals and context
## Key facts and details
## Answers and decisions
## Open items";

const BRIEF_SYSTEM: &str = "You summarize conversations.";

/// Messages of one summarization request.
///
/// `previous` is the summary so far (an earlier compaction, or the previous chunk); it is
/// merged into the new summary. `documents` are attached documents the assistant keeps
/// (re-retrieved each turn), so their contents need no summary.
pub fn summary_messages(
    strategy: Strategy,
    transcript: &str,
    previous: Option<&str>,
    instruction: Option<&str>,
    documents: &[String],
    max_tokens: u64,
) -> Vec<Value> {
    let words = (max_tokens as f64 * 0.6).round().max(40.0) as u64;
    let mut user = String::new();
    match strategy {
        Strategy::Brief => {
            user.push_str("Summarize the following conversation concisely.");
            if let Some(p) = previous.filter(|p| !p.trim().is_empty()) {
                user.push_str(&format!(
                    " Extend the existing summary with it.\n\n<existing_summary>\n{}\n</existing_summary>",
                    p.trim()
                ));
            }
            user.push_str(&format!(
                "\n\n<transcript>\n{}\n</transcript>\n\nWrite at most {words} words.",
                transcript.trim()
            ));
            if let Some(i) = instruction.filter(|i| !i.trim().is_empty()) {
                user.push_str(&format!("\n\nAlso: {}", i.trim()));
            }
            return vec![
                json!({ "role": "system", "content": BRIEF_SYSTEM }),
                json!({ "role": "user", "content": user }),
            ];
        }
        Strategy::Structured | Strategy::Incremental => {}
    }
    user.push_str("Summarize the conversation transcript below.\n\n");
    user.push_str(STRUCTURED_RULES);
    if let Some(p) = previous.filter(|p| !p.trim().is_empty()) {
        user.push_str(&format!(
            "\n\nThe conversation was summarized before. Merge that summary with the new \
             messages into one summary, keeping all of its facts:\n<previous_summary>\n{}\n</previous_summary>",
            p.trim()
        ));
    }
    if !documents.is_empty() {
        user.push_str(&format!(
            "\n\nThese attached documents stay available to the assistant; mention them by \
             name only, do not restate their contents: {}.",
            documents.join(", ")
        ));
    }
    if let Some(i) = instruction.filter(|i| !i.trim().is_empty()) {
        user.push_str(&format!(
            "\n\nAdditional instructions from the user for this summary: {}",
            i.trim()
        ));
    }
    user.push_str(&format!(
        "\n\n<transcript>\n{}\n</transcript>\n\nWrite the summary now (at most about {words} words).",
        transcript.trim()
    ));
    vec![
        json!({ "role": "system", "content": STRUCTURED_SYSTEM }),
        json!({ "role": "user", "content": user }),
    ]
}
