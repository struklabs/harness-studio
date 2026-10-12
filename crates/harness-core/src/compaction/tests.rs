//! Selection, budget and request-building rules, on real `state.db` files (no model needed).
//! The end-to-end run against a real llama-server is in `e2e.rs` (`#[ignore]`).

use super::*;
use crate::state::{AppendMessage, ConversationView, NewConversation, StateDb, VariantInput};
use serde_json::json;
use tempfile::TempDir;

fn open_temp() -> (TempDir, StateDb) {
    let dir = tempfile::tempdir().unwrap();
    let db = StateDb::open(&dir.path().join("state.db")).unwrap();
    (dir, db)
}

struct Conv<'a> {
    db: &'a StateDb,
    id: String,
    last: Option<String>,
}

impl<'a> Conv<'a> {
    fn new(db: &'a StateDb) -> Self {
        let c = db
            .create_conversation(&NewConversation {
                title: Some("t".into()),
                ..Default::default()
            })
            .unwrap();
        Conv {
            db,
            id: c.id,
            last: None,
        }
    }

    fn push(&mut self, role: &str, variant: VariantInput) -> String {
        let m = self
            .db
            .append_message(AppendMessage {
                conversation_id: self.id.clone(),
                parent_id: self.last.clone(),
                role: role.into(),
                variant,
                id: None,
                variant_id: None,
                any_parent_variant: false,
            })
            .unwrap();
        self.last = Some(m.id.clone());
        m.id
    }

    fn say(&mut self, role: &str, text: &str) -> String {
        self.push(
            role,
            VariantInput {
                content: text.into(),
                ..Default::default()
            },
        )
    }

    /// `n` user/assistant turns with recognizable texts.
    fn turns(&mut self, n: usize) {
        for i in 0..n {
            self.say("user", &format!("question {i}"));
            self.say("assistant", &format!("answer {i}"));
        }
    }

    fn marker(&mut self, boundary: &str, summary: &str, undone: bool) -> String {
        let m = test_marker(boundary, undone);
        self.push(
            "system",
            VariantInput {
                content: summary.into(),
                meta: Some(json!({ META_KEY: m })),
                ..Default::default()
            },
        )
    }

    fn view(&self) -> ConversationView {
        self.db.get_conversation(&self.id).unwrap()
    }
}

fn test_marker(boundary: &str, undone: bool) -> Marker {
    Marker {
        version: MARKER_VERSION,
        boundary_id: boundary.into(),
        covered: 2,
        turns: 1,
        kept_turns: 1,
        trigger: TriggerKind::Manual,
        strategy: Strategy::Structured,
        instruction: None,
        model: None,
        tokens_before: 100,
        tokens_after: 40,
        summary_tokens: 20,
        tool_results_cleared: 0,
        exact_tokens: true,
        previous_id: None,
        undone,
        mid_turn: false,
        pinned_id: None,
        created_at: 1,
    }
}

fn roles_and_contents(msgs: &[Value]) -> Vec<(String, String)> {
    msgs.iter()
        .map(|m| {
            (
                m["role"].as_str().unwrap().to_string(),
                m["content"].as_str().unwrap_or_default().to_string(),
            )
        })
        .collect()
}

fn ids(view: &ConversationView, idx: &[usize]) -> Vec<String> {
    idx.iter()
        .map(|&i| view.messages[i].variant.content.clone())
        .collect()
}

// ---- thresholds ----

#[test]
fn percent_threshold_is_a_share_of_the_context() {
    let t = Threshold::Percent { percent: 80.0 };
    assert_eq!(t.limit(4096), 3276);
    assert!(!should_compact(3000, 200, 4096, t));
    assert!(should_compact(3000, 276, 4096, t));
    // Out-of-range percentages are clamped, NaN falls back to 80 %.
    assert_eq!(Threshold::Percent { percent: 150.0 }.limit(1000), 980);
    assert_eq!(Threshold::Percent { percent: 1.0 }.limit(1000), 100);
    assert_eq!(Threshold::Percent { percent: f64::NAN }.limit(1000), 800);
}

#[test]
fn token_threshold_never_exceeds_95_percent_of_the_context() {
    assert_eq!(Threshold::Tokens { tokens: 3000 }.limit(8192), 3000);
    assert_eq!(Threshold::Tokens { tokens: 100_000 }.limit(8192), 7782);
    // Unknown context: no automatic compaction.
    assert!(!should_compact(
        10_000,
        0,
        0,
        Threshold::Tokens { tokens: 10 }
    ));
}

#[test]
fn threshold_round_trips_as_tagged_json() {
    let t: Threshold = serde_json::from_value(json!({ "mode": "tokens", "tokens": 6000 })).unwrap();
    assert_eq!(t, Threshold::Tokens { tokens: 6000 });
    assert_eq!(
        serde_json::to_value(Threshold::default()).unwrap(),
        json!({ "mode": "percent", "percent": 80.0 })
    );
}

// ---- budgets ----

#[test]
fn summary_budget_defaults_to_an_eighth_of_the_context() {
    assert_eq!(summary_budget(4096, None), 512);
    assert_eq!(summary_budget(1024, None), 256);
    assert_eq!(summary_budget(131_072, None), 2048);
    assert_eq!(summary_budget(4096, Some(0)), 512);
    assert_eq!(summary_budget(4096, Some(300)), 300);
    // A requested size can't take more than a third of the context.
    assert_eq!(summary_budget(4096, Some(4000)), 1365);
}

#[test]
fn chunk_budget_leaves_room_for_output_instructions_and_previous_summary() {
    // 4096 - 512 summary - 400 instructions - 300 previous - 204 margin
    assert_eq!(
        chunk_budget(4096, 512, 400, 300, Strategy::Structured).unwrap(),
        2680
    );
    assert_eq!(
        chunk_budget(4096, 512, 400, 300, Strategy::Incremental).unwrap(),
        1024
    );
    let e = chunk_budget(1024, 512, 400, 100, Strategy::Structured).unwrap_err();
    assert_eq!(e.key(), "compaction.context_too_small");
}

#[test]
fn chunks_are_greedy_and_oversized_entries_stand_alone() {
    assert_eq!(chunks(&[100, 100, 100], 250), vec![0..2, 2..3]);
    assert_eq!(chunks(&[100, 900, 100], 250), vec![0..1, 1..2, 2..3]);
    assert_eq!(chunks(&[10, 10], 1000), vec![0..2]);
    assert!(chunks(&[], 100).is_empty());
}

#[test]
fn shorten_keeps_start_and_end() {
    let text = format!("{}{}", "a".repeat(600), "z".repeat(600));
    let s = shorten(&text, 400, 100);
    assert!(s.starts_with("aaa") && s.ends_with("zzz"), "{s}");
    assert!(s.contains("[shortened]"));
    assert!(s.chars().count() < 400);
    assert_eq!(shorten("short", 2, 100), "short");
}

// ---- path analysis ----

#[test]
fn turns_group_tool_rounds_with_their_question() {
    let (_d, db) = open_temp();
    let mut c = Conv::new(&db);
    c.say("user", "q0");
    c.push(
        "assistant",
        VariantInput {
            content: "".into(),
            tool_calls: Some(json!([{ "id": "call_1", "name": "search", "arguments": "{}", "status": "done", "result": "r" }])),
            ..Default::default()
        },
    );
    c.say("assistant", "a0");
    c.say("user", "q1");
    c.say("assistant", "a1");
    let v = c.view();
    assert_eq!(turns(&v.messages, 0), vec![vec![0, 1, 2], vec![3, 4]]);
    assert_eq!(turns(&v.messages, 3), vec![vec![3, 4]]);
}

#[test]
fn plan_keeps_the_newest_turns_and_summarizes_the_rest() {
    let (_d, db) = open_temp();
    let mut c = Conv::new(&db);
    c.turns(5);
    let v = c.view();
    let tokens = vec![50; v.messages.len()];
    let p = plan(
        &v.messages,
        &tokens,
        &PlanOptions {
            n_ctx: 4096,
            keep_turns: 2,
            min_tokens: 0,
        },
    )
    .unwrap();
    assert_eq!(p.turns, 3);
    assert_eq!(p.kept_turns, 2);
    assert_eq!(
        ids(&v, &p.summarize),
        [
            "question 0",
            "answer 0",
            "question 1",
            "answer 1",
            "question 2",
            "answer 2"
        ]
    );
    assert_eq!(
        ids(&v, &p.kept),
        ["question 3", "answer 3", "question 4", "answer 4"]
    );
    assert_eq!(v.messages[p.boundary].variant.content, "answer 2");
    assert_eq!(p.summarize_tokens, 300);
    assert_eq!(p.kept_tokens, 200);
}

#[test]
fn plan_always_summarizes_at_least_one_turn() {
    let (_d, db) = open_temp();
    let mut c = Conv::new(&db);
    c.turns(2);
    let v = c.view();
    let p = plan(
        &v.messages,
        &[10, 10, 10, 10],
        &PlanOptions {
            n_ctx: 4096,
            keep_turns: 5,
            min_tokens: 0,
        },
    )
    .unwrap();
    assert_eq!((p.turns, p.kept_turns), (1, 1));
    // A single turn is summarized as a whole.
    let mut c2 = Conv::new(&db);
    c2.turns(1);
    let v2 = c2.view();
    let p2 = plan(
        &v2.messages,
        &[10, 10],
        &PlanOptions {
            n_ctx: 4096,
            keep_turns: 2,
            min_tokens: 0,
        },
    )
    .unwrap();
    assert_eq!((p2.turns, p2.kept_turns), (1, 0));
}

#[test]
fn kept_turns_shrink_when_they_fill_too_much_of_the_context() {
    let (_d, db) = open_temp();
    let mut c = Conv::new(&db);
    c.turns(4);
    let v = c.view();
    // The two newest turns would take 2 × 900 tokens > 40 % of 4096 (1638).
    let tokens = vec![450; v.messages.len()];
    let p = plan(
        &v.messages,
        &tokens,
        &PlanOptions {
            n_ctx: 4096,
            keep_turns: 2,
            min_tokens: 0,
        },
    )
    .unwrap();
    assert_eq!(p.kept_turns, 1);
    assert_eq!(p.turns, 3);
}

#[test]
fn automatic_compaction_needs_a_worthwhile_amount() {
    let (_d, db) = open_temp();
    let mut c = Conv::new(&db);
    c.turns(3);
    let v = c.view();
    let tokens = vec![40; v.messages.len()];
    let e = plan(
        &v.messages,
        &tokens,
        &PlanOptions {
            n_ctx: 4096,
            keep_turns: 2,
            min_tokens: 409,
        },
    )
    .unwrap_err();
    assert_eq!(e.key(), "compaction.too_little");
    let e = plan(
        &[],
        &[],
        &PlanOptions {
            n_ctx: 4096,
            keep_turns: 2,
            min_tokens: 0,
        },
    )
    .unwrap_err();
    assert_eq!(e.key(), "compaction.nothing_to_compact");
}

#[test]
fn request_uses_summary_and_messages_after_the_boundary() {
    let (_d, db) = open_temp();
    let mut c = Conv::new(&db);
    c.turns(3);
    let boundary = c.view().messages[3].id.clone(); // "answer 1"
    c.marker(&boundary, "SUMMARY-1", false);
    c.say("user", "question 3");
    let v = c.view();

    let a = active_compaction(&v.messages).unwrap();
    assert_eq!((a.boundary_index, a.marker_index), (3, 6));
    let msgs = roles_and_contents(&request_messages(&v.messages, "Be brief."));
    assert_eq!(msgs[0].0, "system");
    assert!(msgs[0].1.starts_with("Be brief.\n\n<conversation_summary>"));
    assert!(msgs[0].1.contains("SUMMARY-1"));
    assert_eq!(
        msgs[1..].to_vec(),
        vec![
            ("user".to_string(), "question 2".to_string()),
            ("assistant".to_string(), "answer 2".to_string()),
            ("user".to_string(), "question 3".to_string()),
        ]
    );
    // No system prompt: the summary alone is the system message.
    let msgs = request_messages(&v.messages, "  ");
    assert!(msgs[0]["content"]
        .as_str()
        .unwrap()
        .starts_with("<conversation_summary>"));
}

#[test]
fn undone_marker_is_ignored_and_pauses_auto_compaction() {
    let (_d, db) = open_temp();
    let mut c = Conv::new(&db);
    c.turns(2);
    let boundary = c.view().messages[1].id.clone();
    let marker = c.marker(&boundary, "S", false);
    c.say("user", "q");
    assert!(!auto_paused(&c.view().messages));

    let v = set_undone(&db, &marker, true).unwrap();
    assert!(active_compaction(&v.messages).is_none());
    assert!(auto_paused(&v.messages));
    let msgs = request_messages(&v.messages, "");
    assert_eq!(msgs.len(), 5, "full history: {msgs:?}");
    assert_eq!(msgs[0]["content"], "question 0");

    let v = set_undone(&db, &marker, false).unwrap();
    assert!(active_compaction(&v.messages).is_some());
    assert_eq!(request_messages(&v.messages, "").len(), 4);

    let user = v.messages[0].id.clone();
    assert_eq!(
        set_undone(&db, &user, true).unwrap_err(),
        String::from(err("compaction.not_a_marker").with("id", &user))
    );
}

#[test]
fn undoing_a_newer_marker_falls_back_to_the_older_one() {
    let (_d, db) = open_temp();
    let mut c = Conv::new(&db);
    c.turns(2);
    let b1 = c.view().messages[1].id.clone();
    c.marker(&b1, "OLD", false);
    c.turns(2);
    let b2 = c.view().messages[6].id.clone(); // "answer 0" of the second batch
    let m2 = c.marker(&b2, "NEW", false);
    let v = c.view();
    assert_eq!(active_compaction(&v.messages).unwrap().summary, "NEW");
    let v = set_undone(&db, &m2, true).unwrap();
    let a = active_compaction(&v.messages).unwrap();
    assert_eq!(a.summary, "OLD");
    // Every marker is skipped in the request; the old boundary applies.
    let msgs = roles_and_contents(&request_messages(&v.messages, ""));
    assert!(msgs[0].1.contains("OLD") && !msgs[0].1.contains("NEW"));
    // After the old boundary ("answer 0"): q1, a1 and the four messages of the second batch.
    assert_eq!(msgs.len(), 1 + 6);
    assert!(msgs
        .iter()
        .all(|(r, _)| r == "system" || r == "user" || r == "assistant"));
}

#[test]
fn a_branch_made_before_the_marker_has_the_full_history() {
    let (_d, db) = open_temp();
    let mut c = Conv::new(&db);
    c.turns(3);
    let v = c.view();
    let boundary = v.messages[3].id.clone();
    let kept_question = v.messages[4].id.clone();
    c.marker(&boundary, "S", false);
    // Editing a kept question makes a sibling branch that never saw the marker.
    let edited = db
        .edit_user_message(&kept_question, "question 2 (edited)", None)
        .unwrap();
    let v = db.get_conversation(&c.id).unwrap();
    assert_eq!(v.messages.last().unwrap().id, edited.id);
    assert!(active_compaction(&v.messages).is_none());
    assert_eq!(request_messages(&v.messages, "").len(), 5);
}

#[test]
fn tool_rounds_are_kept_whole_without_dangling_ids() {
    let (_d, db) = open_temp();
    let mut c = Conv::new(&db);
    c.turns(1);
    c.say("user", "use the tool");
    c.push(
        "assistant",
        VariantInput {
            tool_calls: Some(json!([
                { "id": "call_a", "name": "read_file", "arguments": "{\"path\":\"a\"}", "status": "done", "result": "A" },
                { "id": "call_b", "name": "rm", "arguments": "{}", "status": "denied" }
            ])),
            ..Default::default()
        },
    );
    c.say("assistant", "done");
    let v = c.view();
    let p = plan(
        &v.messages,
        &vec![10; v.messages.len()],
        &PlanOptions {
            n_ctx: 4096,
            keep_turns: 1,
            min_tokens: 0,
        },
    )
    .unwrap();
    assert_eq!(ids(&v, &p.summarize), ["question 0", "answer 0"]);
    let boundary = v.messages[p.boundary].id.clone();
    c.marker(&boundary, "S", false);
    let msgs = request_messages(&c.view().messages, "");
    let call_ids: Vec<&str> = msgs
        .iter()
        .filter_map(|m| m["tool_calls"].as_array())
        .flatten()
        .filter_map(|c| c["id"].as_str())
        .collect();
    let answered: Vec<&str> = msgs
        .iter()
        .filter_map(|m| m["tool_call_id"].as_str())
        .collect();
    assert_eq!(call_ids, ["call_a", "call_b"]);
    assert_eq!(answered, call_ids);
    assert_eq!(msgs.last().unwrap()["content"], "done");
    let denied = msgs.iter().find(|m| m["tool_call_id"] == "call_b").unwrap();
    assert_eq!(denied["content"], DENIED_TEXT);
}

#[test]
fn transcript_clears_long_tool_results_only_when_asked() {
    let (_d, db) = open_temp();
    let mut c = Conv::new(&db);
    c.say("user", "q");
    let long = "x".repeat(2000);
    c.push(
        "assistant",
        VariantInput {
            content: "Let me look.".into(),
            tool_calls: Some(json!([{ "id": "c1", "name": "fetch", "arguments": "{\"url\":\"u\"}", "status": "done", "result": long }])),
            ..Default::default()
        },
    );
    let v = c.view();
    let (full, n) = transcript_entry(&v.messages[1], false);
    assert_eq!(n, 0);
    assert!(full.contains(&"x".repeat(2000)));
    let (short, n) = transcript_entry(&v.messages[1], true);
    assert_eq!(n, 1);
    assert!(short.starts_with(
        "Assistant: Let me look.\nAssistant called tool `fetch` with {\"url\":\"u\"}"
    ));
    assert!(
        short.contains("[rest of the tool result cleared, 1700 characters]"),
        "{short}"
    );
    assert_eq!(transcript_entry(&v.messages[0], true).0, "User: q");
}

#[test]
fn markers_survive_fork_with_new_ids() {
    let (_d, db) = open_temp();
    let mut c = Conv::new(&db);
    c.turns(2);
    let boundary = c.view().messages[1].id.clone();
    c.marker(&boundary, "S", false);
    let tail = c.say("user", "next");
    let fork = db.fork_conversation(&tail, None).unwrap();
    let v = db.get_conversation(&fork.id).unwrap();
    assert_ne!(v.messages[1].id, boundary, "a fork has new message ids");
    let a = active_compaction(&v.messages).expect("marker resolves in the fork");
    assert_eq!(a.boundary_index, 1);
    assert_eq!(a.summary, "S");
    // Duplicate and JSON export/import go through the same copy.
    let dup = db.duplicate_conversation(&c.id, None).unwrap();
    let v = db.get_conversation(&dup.id).unwrap();
    assert!(active_compaction(&v.messages).is_some());
}

#[test]
fn strip_think_removes_reasoning_blocks() {
    assert_eq!(strip_think("<think>hmm</think>\n## Facts"), "## Facts");
    assert_eq!(strip_think("plain"), "plain");
}

#[test]
fn prompts_carry_previous_summary_documents_and_instruction() {
    let m = summary_messages(
        Strategy::Structured,
        "User: hi",
        Some("OLD"),
        Some("keep numbers"),
        &["a.pdf".into()],
        500,
    );
    let user = m[1]["content"].as_str().unwrap();
    assert!(user.contains("<previous_summary>\nOLD"));
    assert!(user.contains("a.pdf"));
    assert!(user.contains("keep numbers"));
    assert!(user.contains("<transcript>\nUser: hi\n</transcript>"));
    assert!(user.contains("300 words"));
    let b = summary_messages(Strategy::Brief, "User: hi", None, None, &[], 100);
    assert!(b[1]["content"]
        .as_str()
        .unwrap()
        .starts_with("Summarize the following conversation concisely."));
}

#[test]
fn a_summary_that_does_not_shrink_the_request_is_refused() {
    assert!(require_gain(2108, 1353).is_ok());
    let e = require_gain(2108, 2243).unwrap_err();
    assert_eq!(e.key(), "compaction.no_gain");
    assert!(require_gain(1000, 1000).is_err());
}

// ---- compaction between two tool rounds of a running answer ----

fn tool_round(c: &mut Conv, text: &str, id: &str, result: &str) -> String {
    c.push(
        "assistant",
        VariantInput {
            content: text.into(),
            tool_calls: Some(json!([
                { "id": id, "name": "read_file", "arguments": "{}", "status": "done", "result": result }
            ])),
            ..Default::default()
        },
    )
}

fn mid_opts() -> PlanOptions {
    PlanOptions {
        n_ctx: 4096,
        keep_turns: 2,
        min_tokens: 0,
    }
}

#[test]
fn mid_turn_plan_summarizes_early_rounds_and_pins_the_question() {
    let (_d, db) = open_temp();
    let mut c = Conv::new(&db);
    c.turns(2);
    c.say("user", "research everything");
    tool_round(&mut c, "round one", "c1", "R1");
    tool_round(&mut c, "round two", "c2", "R2");
    tool_round(&mut c, "round three", "c3", "R3");
    let v = c.view();
    let tokens = vec![40; v.messages.len()];
    let p = plan_mid_turn(&v.messages, &tokens, &mid_opts()).unwrap();
    // Older turns and the first rounds are summarized; the question is pinned, not summarized.
    assert_eq!(
        ids(&v, &p.summarize),
        [
            "question 0",
            "answer 0",
            "question 1",
            "answer 1",
            "round one",
            "round two"
        ]
    );
    assert_eq!(ids(&v, &[p.pinned.unwrap()]), ["research everything"]);
    assert_eq!(ids(&v, &p.kept), ["round three"]);
    assert_eq!(v.messages[p.boundary].variant.content, "round two");
    assert_eq!(p.pinned_tokens, 40);
    assert_eq!(p.kept_tokens, 40);

    // The request: summary, the pinned question, then the last round with its tool result.
    let mut marker = test_marker(&v.messages[p.boundary].id, false);
    marker.mid_turn = true;
    marker.pinned_id = Some(v.messages[p.pinned.unwrap()].id.clone());
    c.push(
        "system",
        VariantInput {
            content: "S".into(),
            meta: Some(json!({ META_KEY: marker })),
            ..Default::default()
        },
    );
    let msgs = request_messages(&c.view().messages, "sys");
    let roles: Vec<&str> = msgs.iter().map(|m| m["role"].as_str().unwrap()).collect();
    assert_eq!(roles, ["system", "user", "assistant", "tool"]);
    assert_eq!(msgs[1]["content"], "research everything");
    assert_eq!(msgs[3]["content"], "R3");
    assert!(msgs[0]["content"].as_str().unwrap().contains('S'));
}

#[test]
fn mid_turn_with_one_round_only_summarizes_earlier_turns() {
    let (_d, db) = open_temp();
    let mut c = Conv::new(&db);
    c.turns(2);
    c.say("user", "now this");
    tool_round(&mut c, "only round", "c1", "R1");
    let v = c.view();
    let p = plan_mid_turn(&v.messages, &vec![40; v.messages.len()], &mid_opts()).unwrap();
    assert_eq!(
        ids(&v, &p.summarize),
        ["question 0", "answer 0", "question 1", "answer 1"]
    );
    assert_eq!(ids(&v, &p.kept), ["now this", "only round"]);
    assert!(p.pinned.is_none());

    // A single turn with a single round has nothing to compact.
    let (_d2, db2) = open_temp();
    let mut c2 = Conv::new(&db2);
    c2.say("user", "q");
    tool_round(&mut c2, "r", "c1", "R");
    let v2 = c2.view();
    let e = plan_mid_turn(&v2.messages, &[40; 2], &mid_opts()).unwrap_err();
    assert_eq!(e.key(), "compaction.nothing_to_compact");
}

#[test]
fn a_second_mid_turn_compaction_keeps_the_pinned_question() {
    let (_d, db) = open_temp();
    let mut c = Conv::new(&db);
    c.say("user", "long task");
    tool_round(&mut c, "r1", "c1", "R1");
    let r2 = tool_round(&mut c, "r2", "c2", "R2");
    let pinned = c.view().messages[0].id.clone();
    let mut m = test_marker(&r2, false);
    m.mid_turn = true;
    m.pinned_id = Some(pinned.clone());
    c.push(
        "system",
        VariantInput {
            content: "S1".into(),
            meta: Some(json!({ META_KEY: m })),
            ..Default::default()
        },
    );
    tool_round(&mut c, "r3", "c3", "R3");
    tool_round(&mut c, "r4", "c4", "R4");
    let v = c.view();
    let a = active_compaction(&v.messages).unwrap();
    assert_eq!(a.pinned_index, Some(0));
    let mut tokens = vec![0; v.messages.len()];
    for (i, m) in v.messages.iter().enumerate() {
        if is_chat(m) {
            tokens[i] = 30;
        }
    }
    let p = plan_mid_turn(&v.messages, &tokens, &mid_opts()).unwrap();
    // r3 is summarized (with the earlier summary folded in), r4 stays, the question is carried.
    assert_eq!(ids(&v, &p.summarize), ["r3"]);
    assert_eq!(ids(&v, &p.kept), ["r4"]);
    assert_eq!(p.pinned, Some(0));
    assert_eq!(p.previous.as_ref().unwrap().summary, "S1");
}

#[test]
fn pinned_ids_follow_a_copied_conversation() {
    let (_d, db) = open_temp();
    let mut c = Conv::new(&db);
    c.say("user", "long task");
    tool_round(&mut c, "r1", "c1", "R1");
    let r2 = tool_round(&mut c, "r2", "c2", "R2");
    let pinned = c.view().messages[0].id.clone();
    let mut m = test_marker(&r2, false);
    m.mid_turn = true;
    m.pinned_id = Some(pinned);
    c.push(
        "system",
        VariantInput {
            content: "S".into(),
            meta: Some(json!({ META_KEY: m })),
            ..Default::default()
        },
    );
    tool_round(&mut c, "r3", "c3", "R3");
    let copy = db.duplicate_conversation(&c.id, None).unwrap();
    let v = db.get_conversation(&copy.id).unwrap();
    let a = active_compaction(&v.messages).expect("the copy keeps its active marker");
    assert_eq!(a.pinned_index, Some(0));
    let msgs = request_messages(&v.messages, "");
    assert_eq!(msgs[1]["content"], "long task");
}

#[test]
fn earlier_rounds_in_variant_meta_become_request_messages() {
    let (_d, db) = open_temp();
    let mut c = Conv::new(&db);
    c.say("user", "q");
    // One answer (a split-view side) whose first round is stored in `meta.rounds`.
    c.push(
        "assistant",
        VariantInput {
            content: "final".into(),
            meta: Some(json!({ "rounds": [
                { "content": "step", "tool_calls": [
                    { "id": "k1", "name": "t", "arguments": "{}", "status": "done", "result": "RES" }
                ]}
            ]})),
            ..Default::default()
        },
    );
    let v = c.view();
    let msgs = request_messages(&v.messages, "");
    let roles: Vec<&str> = msgs.iter().map(|m| m["role"].as_str().unwrap()).collect();
    assert_eq!(roles, ["user", "assistant", "tool", "assistant"]);
    assert_eq!(msgs[2]["content"], "RES");
    assert_eq!(msgs[3]["content"], "final");
    let text = message_text(&v.messages[1]);
    assert!(text.contains("step") && text.contains("RES") && text.contains("final"));
    let (entry, _) = transcript_entry(&v.messages[1], true);
    assert!(entry.contains("Assistant called tool `t`") && entry.ends_with("Assistant: final"));
}

#[tokio::test]
async fn next_request_counts_rendered_template_tools_and_generation_prefix() {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let body = json!({"messages":[{"role":"system","content":"system"},{"role":"user","content":"draft"}],"tools":[{"type":"function","function":{"name":"test"}}],"chat_template_kwargs":{"enable_thinking":false}});
    let expected = body.clone();
    let server = tokio::spawn(async move {
        for route in ["apply-template", "tokenize"] {
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut raw = Vec::new();
            loop {
                let mut buf = [0; 8192];
                let n = socket.read(&mut buf).await.unwrap();
                assert!(n > 0);
                raw.extend_from_slice(&buf[..n]);
                if let Some(at) = raw.windows(4).position(|w| w == b"\r\n\r\n") {
                    let head = String::from_utf8_lossy(&raw[..at]).to_lowercase();
                    let len: usize = head
                        .lines()
                        .find_map(|l| l.strip_prefix("content-length:"))
                        .unwrap()
                        .trim()
                        .parse()
                        .unwrap();
                    if raw.len() < at + 4 + len {
                        continue;
                    }
                    assert!(head.starts_with(&format!("post /{route} ")));
                    let sent: serde_json::Value = serde_json::from_slice(&raw[at + 4..]).unwrap();
                    let reply = if route == "apply-template" {
                        assert_eq!(sent["messages"], expected["messages"]);
                        assert_eq!(sent["tools"], expected["tools"]);
                        assert_eq!(
                            sent["chat_template_kwargs"],
                            expected["chat_template_kwargs"]
                        );
                        assert_eq!(sent["add_generation_prompt"], true);
                        json!({"prompt":"<system>system<tools>test<user>draft<assistant>"})
                    } else {
                        assert_eq!(
                            sent["content"],
                            "<system>system<tools>test<user>draft<assistant>"
                        );
                        assert_eq!(sent["add_special"], false);
                        json!({"tokens":[1,2,3,4,5,6,7]})
                    }
                    .to_string();
                    let response=format!("HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{reply}",reply.len());
                    socket.write_all(response.as_bytes()).await.unwrap();
                    break;
                }
            }
        }
    });
    let ep = TokenizeEndpoint {
        host_url: format!("http://{address}"),
        headers: Default::default(),
    };
    assert_eq!(count_request(Some(&ep), &body).await.unwrap(), (7, true));
    server.await.unwrap();
    let (n, exact) = count_request(None, &body).await.unwrap();
    assert!(n > 0);
    assert!(!exact);
}
