//! Tests against real SQLite files in temporary directories.

use super::*;
use serde_json::json;
use tempfile::TempDir;

fn open_temp() -> (TempDir, StateDb) {
    let dir = tempfile::tempdir().unwrap();
    let db = StateDb::open(&dir.path().join("state.db")).unwrap();
    (dir, db)
}

fn text(content: &str) -> VariantInput {
    VariantInput {
        content: content.to_string(),
        ..Default::default()
    }
}

fn new_conv(db: &StateDb, title: &str) -> Conversation {
    db.create_conversation(&NewConversation {
        title: Some(title.to_string()),
        ..Default::default()
    })
    .unwrap()
}

fn append(
    db: &StateDb,
    conv: &str,
    parent: Option<&str>,
    role: &str,
    content: &str,
) -> PathMessage {
    db.append_message(AppendMessage {
        conversation_id: conv.to_string(),
        parent_id: parent.map(str::to_string),
        role: role.to_string(),
        variant: text(content),
        id: None,
        variant_id: None,
        any_parent_variant: false,
    })
    .unwrap()
}

fn contents(view: &ConversationView) -> Vec<String> {
    view.messages
        .iter()
        .map(|m| m.variant.content.clone())
        .collect()
}

#[test]
fn migrations_are_idempotent_and_set_user_version() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("state.db");
    {
        let db = StateDb::open(&path).unwrap();
        assert_eq!(db.user_version().unwrap(), SCHEMA_VERSION);
        new_conv(&db, "kalıcı");
    }
    // Re-opening applies nothing and keeps the data.
    let db = StateDb::open(&path).unwrap();
    assert_eq!(db.user_version().unwrap(), SCHEMA_VERSION);
    assert!(!db.is_read_only());
    let list = db.list_conversations(&ListQuery::default()).unwrap();
    assert_eq!(list.len(), 1);
    assert_eq!(list[0].title, "kalıcı");
}

#[test]
fn bundled_sqlite_has_fts5_and_pragmas() {
    let (_d, db) = open_temp();
    db.read(|c| {
        let mode: String = c.query_row("PRAGMA journal_mode", [], |r| r.get(0))?;
        assert_eq!(mode.to_lowercase(), "wal");
        let fk: i64 = c.query_row("PRAGMA foreign_keys", [], |r| r.get(0))?;
        assert_eq!(fk, 1);
        let fts: i64 = c.query_row("SELECT sqlite_compileoption_used('ENABLE_FTS5')", [], |r| {
            r.get(0)
        })?;
        assert_eq!(fts, 1);
        Ok(())
    })
    .unwrap();
}

#[test]
fn newer_schema_opens_read_only_and_refuses_writes() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("state.db");
    {
        let db = StateDb::open(&path).unwrap();
        new_conv(&db, "eski");
    }
    {
        let conn = rusqlite::Connection::open(&path).unwrap();
        conn.pragma_update(None, "user_version", SCHEMA_VERSION + 5)
            .unwrap();
    }
    let db = StateDb::open(&path).unwrap();
    assert!(db.is_read_only());
    // Reads still work, the version is untouched.
    assert_eq!(
        db.list_conversations(&ListQuery::default()).unwrap().len(),
        1
    );
    assert_eq!(db.user_version().unwrap(), SCHEMA_VERSION + 5);
    let err = db
        .create_conversation(&NewConversation::default())
        .unwrap_err();
    assert!(matches!(err, StateError::ReadOnly { found } if found == SCHEMA_VERSION + 5));
    let text = err.to_string();
    let (key, params) = crate::error_key::parse(&text).unwrap();
    assert_eq!(key, "state.read_only");
    assert_eq!(params["found"], SCHEMA_VERSION + 5);
    assert_eq!(params["supported"], SCHEMA_VERSION);
    drop(db);
    let conn = rusqlite::Connection::open(&path).unwrap();
    let v: i64 = conn
        .query_row("PRAGMA user_version", [], |r| r.get(0))
        .unwrap();
    assert_eq!(v, SCHEMA_VERSION + 5);
}

#[test]
fn conversation_crud_folders_and_listing() {
    let (_d, db) = open_temp();
    let a = new_conv(&db, "Birinci");
    let b = new_conv(&db, "İkinci");
    let c = new_conv(&db, "Üçüncü");
    // updated_at has millisecond resolution; make the append clearly newer.
    std::thread::sleep(std::time::Duration::from_millis(5));
    append(&db, &a.id, None, "user", "merhaba");

    // Most recently updated first; pinned before everything.
    db.set_pinned(&c.id, true).unwrap();
    let ids: Vec<String> = db
        .list_conversations(&ListQuery::default())
        .unwrap()
        .into_iter()
        .map(|s| s.id)
        .collect();
    assert_eq!(ids, vec![c.id.clone(), a.id.clone(), b.id.clone()]);

    db.set_favorite(&b.id, true).unwrap();
    let favs = db
        .list_conversations(&ListQuery {
            favorites_only: true,
            ..Default::default()
        })
        .unwrap();
    assert_eq!(favs.len(), 1);
    assert_eq!(favs[0].id, b.id);

    assert!(db.set_title(&b.id, "Yeni ad", "user", None).unwrap());
    // An auto title must not replace a user title.
    assert!(!db
        .set_title(&b.id, "Otomatik", "auto", Some("fallback"))
        .unwrap());
    assert_eq!(
        db.get_conversation(&b.id).unwrap().conversation.title,
        "Yeni ad"
    );

    // Folders: move, list by folder, delete moves conversations back to the parent.
    let parent = db.create_folder("Projeler", None).unwrap();
    let child = db.create_folder("Harness", Some(&parent.id)).unwrap();
    db.move_to_folder(&a.id, Some(&child.id)).unwrap();
    let in_child = db
        .list_conversations(&ListQuery {
            folder_id: Some(child.id.clone()),
            ..Default::default()
        })
        .unwrap();
    assert_eq!(in_child.len(), 1);
    let unfiled = db
        .list_conversations(&ListQuery {
            unfiled: true,
            ..Default::default()
        })
        .unwrap();
    assert_eq!(unfiled.len(), 2);
    db.rename_folder(&child.id, "Harness Studio").unwrap();
    db.delete_folder(&child.id).unwrap();
    assert_eq!(
        db.get_conversation(&a.id).unwrap().conversation.folder_id,
        Some(parent.id.clone())
    );
    assert_eq!(db.list_folders().unwrap().len(), 1);
    assert!(db.move_to_folder(&a.id, Some("yok")).is_err());

    // Settings snapshot round-trip.
    let updated = db
        .update_conversation(
            &a.id,
            &ConversationPatch {
                system_prompt: Some("Kısa yanıt ver.".into()),
                settings: Some(json!({ "temperature": 0.3 })),
                model_alias: Some("qwen".into()),
                ..Default::default()
            },
        )
        .unwrap();
    assert_eq!(updated.system_prompt, "Kısa yanıt ver.");
    assert_eq!(updated.settings["temperature"], json!(0.3));
    assert_eq!(updated.model_alias.as_deref(), Some("qwen"));

    // Last opened.
    assert_eq!(db.last_opened_conversation().unwrap(), None);
    db.open_conversation(&b.id).unwrap();
    assert_eq!(db.last_opened_conversation().unwrap(), Some(b.id.clone()));

    // Delete cascades to messages and variants.
    db.delete_conversation(&a.id).unwrap();
    assert!(matches!(
        db.get_conversation(&a.id),
        Err(StateError::NotFound(_))
    ));
    let (msgs, vars): (i64, i64) = db
        .read(|c| {
            Ok((
                c.query_row("SELECT COUNT(*) FROM messages", [], |r| r.get(0))?,
                c.query_row("SELECT COUNT(*) FROM message_variants", [], |r| r.get(0))?,
            ))
        })
        .unwrap();
    assert_eq!((msgs, vars), (0, 0));
}

#[test]
fn path_follows_current_leaf_and_regenerate_adds_variants() {
    let (_d, db) = open_temp();
    let conv = new_conv(&db, "Dallar");
    let u1 = append(&db, &conv.id, None, "user", "soru 1");
    let a1 = append(&db, &conv.id, Some(&u1.id), "assistant", "cevap 1");
    let u2 = append(&db, &conv.id, Some(&a1.id), "user", "soru 2");
    let a2 = append(&db, &conv.id, Some(&u2.id), "assistant", "cevap 2");
    assert_eq!(
        a2.parent_variant_id.as_deref(),
        Some(u2.variant.id.as_str())
    );
    let view = db.get_conversation(&conv.id).unwrap();
    assert_eq!(contents(&view), ["soru 1", "cevap 1", "soru 2", "cevap 2"]);

    // Regenerate the first answer: new variant, path is cut at that message.
    let regen = db.add_variant(&a1.id, &text("cevap 1b"), None).unwrap();
    assert_eq!(regen.variant_ids.len(), 2);
    assert_eq!(regen.variant_index, 1);
    let view = db.get_conversation(&conv.id).unwrap();
    assert_eq!(contents(&view), ["soru 1", "cevap 1b"]);

    // Continue under the new variant.
    let u2b = append(&db, &conv.id, Some(&a1.id), "user", "soru 2b");
    assert_eq!(
        u2b.parent_variant_id.as_deref(),
        Some(regen.variant.id.as_str())
    );
    // The old continuation is not a sibling of the new one (different parent variant).
    assert_eq!(u2b.sibling_ids.len(), 1);

    // Switching back to the first variant restores its continuation.
    let view = db.select_variant(&a1.id, &a1.variant.id).unwrap();
    assert_eq!(contents(&view), ["soru 1", "cevap 1", "soru 2", "cevap 2"]);
    assert_eq!(view.messages[1].variant_ids.len(), 2);
    assert_eq!(view.messages[1].variant_index, 0);

    let view = db.select_variant(&a1.id, &regen.variant.id).unwrap();
    assert_eq!(contents(&view), ["soru 1", "cevap 1b", "soru 2b"]);

    // update_variant replaces the content (tool results arriving later).
    db.update_variant(
        &a2.variant.id,
        &VariantInput {
            content: "cevap 2 (araçlı)".into(),
            tool_calls: Some(json!([{ "id": "c1", "name": "read_file", "arguments": "{}", "status": "done", "result": "ok" }])),
            stats: Some(json!({ "source": "timings", "predictedN": 12 })),
            finish_reason: Some("stop".into()),
            ..Default::default()
        },
    )
    .unwrap();
    let view = db.select_branch(&u2.id).unwrap();
    let last = view.messages.last().unwrap();
    assert_eq!(last.variant.content, "cevap 2 (araçlı)");
    assert_eq!(
        last.variant.tool_calls.as_ref().unwrap()[0]["name"],
        "read_file"
    );
    assert_eq!(last.variant.stats.as_ref().unwrap()["predictedN"], 12);
}

#[test]
fn edit_user_message_creates_sibling_branch() {
    let (_d, db) = open_temp();
    let conv = new_conv(&db, "Düzenleme");
    let u1 = append(&db, &conv.id, None, "user", "ilk");
    let a1 = append(&db, &conv.id, Some(&u1.id), "assistant", "yanıt");
    let u2 = append(&db, &conv.id, Some(&a1.id), "user", "yanlış soru");
    append(&db, &conv.id, Some(&u2.id), "assistant", "yanlış yanıt");

    let edited = db.edit_user_message(&u2.id, "doğru soru", None).unwrap();
    assert_eq!(edited.parent_id.as_deref(), Some(a1.id.as_str()));
    assert_eq!(edited.sibling_ids, vec![u2.id.clone(), edited.id.clone()]);
    assert_eq!(edited.sibling_index, 1);
    append(&db, &conv.id, Some(&edited.id), "assistant", "doğru yanıt");
    let view = db.get_conversation(&conv.id).unwrap();
    assert_eq!(
        contents(&view),
        ["ilk", "yanıt", "doğru soru", "doğru yanıt"]
    );

    // Branch switcher: back to the original question and its answer.
    let view = db.select_branch(&u2.id).unwrap();
    assert_eq!(
        contents(&view),
        ["ilk", "yanıt", "yanlış soru", "yanlış yanıt"]
    );
    assert_eq!(view.messages[2].sibling_index, 0);
    assert_eq!(view.messages[2].sibling_ids.len(), 2);

    // Assistant messages cannot be "edited" into a branch.
    assert!(db.edit_user_message(&a1.id, "x", None).is_err());

    // Root-level edits work too.
    let root_edit = db
        .edit_user_message(&u1.id, "başka başlangıç", None)
        .unwrap();
    assert_eq!(root_edit.parent_id, None);
    assert_eq!(root_edit.sibling_ids.len(), 2);
}

#[test]
fn fork_copies_the_path_and_duplicate_copies_everything() {
    let (_d, db) = open_temp();
    let conv = new_conv(&db, "Kaynak");
    db.update_conversation(
        &conv.id,
        &ConversationPatch {
            system_prompt: Some("sistem".into()),
            ..Default::default()
        },
    )
    .unwrap();
    let u1 = append(&db, &conv.id, None, "user", "a");
    let a1 = append(&db, &conv.id, Some(&u1.id), "assistant", "b");
    db.add_variant(&a1.id, &text("b2"), None).unwrap();
    let u2 = append(&db, &conv.id, Some(&a1.id), "user", "c");
    append(&db, &conv.id, Some(&u2.id), "assistant", "d");

    let fork = db.fork_conversation(&a1.id, Some("Çatal")).unwrap();
    assert_eq!(
        fork.forked_from_conversation_id.as_deref(),
        Some(conv.id.as_str())
    );
    assert_eq!(fork.forked_from_message_id.as_deref(), Some(a1.id.as_str()));
    assert_eq!(fork.system_prompt, "sistem");
    let view = db.get_conversation(&fork.id).unwrap();
    // The forked path uses the variant that was shown (b2), one variant per message.
    assert_eq!(contents(&view), ["a", "b2"]);
    assert_eq!(view.messages[1].variant_ids.len(), 1);
    assert_ne!(view.messages[0].id, u1.id);

    let dup = db.duplicate_conversation(&conv.id, Some("Kopya")).unwrap();
    let orig_view = db.get_conversation(&conv.id).unwrap();
    let dup_view = db.get_conversation(&dup.id).unwrap();
    assert_eq!(contents(&orig_view), contents(&dup_view));
    assert_eq!(dup_view.messages[1].variant_ids.len(), 2);
    let count = |id: &str| -> i64 {
        db.read(|c| {
            Ok(c.query_row(
                "SELECT COUNT(*) FROM messages WHERE conversation_id = ?1",
                [id],
                |r| r.get(0),
            )?)
        })
        .unwrap()
    };
    assert_eq!(count(&conv.id), count(&dup.id));
    assert_eq!(dup.title, "Kopya");
}

#[test]
fn full_text_search_tracks_titles_and_variants() {
    let (_d, db) = open_temp();
    let a = new_conv(&db, "Rust öğreniyorum");
    let b = new_conv(&db, "Tatil planı");
    let ua = append(
        &db,
        &a.id,
        None,
        "user",
        "Ömür boyu sahiplik kuralları nedir?",
    );
    append(
        &db,
        &b.id,
        None,
        "user",
        "Kapadokya için balon turu önerisi",
    );

    let ids = |q: &str| -> Vec<String> {
        db.list_conversations(&ListQuery {
            query: Some(q.into()),
            ..Default::default()
        })
        .unwrap()
        .into_iter()
        .map(|s| s.id)
        .collect()
    };
    assert_eq!(ids("rust"), vec![a.id.clone()]);
    // Prefix + diacritic-insensitive matching.
    assert_eq!(ids("kapad"), vec![b.id.clone()]);
    assert_eq!(ids("omur"), vec![a.id.clone()]);
    assert!(ids("\"; DROP TABLE x --").is_empty());
    assert_eq!(
        db.list_conversations(&ListQuery {
            query: Some("balon".into()),
            ..Default::default()
        })
        .unwrap()[0]
            .snippet
            .as_deref()
            .map(|s| s.contains("balon")),
        Some(true)
    );

    // Title updates and variant updates are re-indexed.
    db.set_title(&b.id, "Yaz tatili", "user", None).unwrap();
    assert!(ids("plan").is_empty());
    assert_eq!(ids("yaz"), vec![b.id.clone()]);
    db.update_variant(&ua.variant.id, &text("borrow checker"))
        .unwrap();
    assert!(ids("sahiplik").is_empty());
    assert_eq!(ids("borrow"), vec![a.id.clone()]);

    // Cascaded deletes clean up the index.
    db.delete_conversation(&a.id).unwrap();
    assert!(ids("borrow").is_empty());
    let rows: i64 = db
        .read(|c| Ok(c.query_row("SELECT COUNT(*) FROM search_index", [], |r| r.get(0))?))
        .unwrap();
    assert_eq!(rows, 2); // title + one variant of b
}

#[test]
fn export_import_round_trip_keeps_tree_and_timestamps() {
    let (_d, db) = open_temp();
    let conv = new_conv(&db, "Dışa aktar");
    db.update_conversation(
        &conv.id,
        &ConversationPatch {
            system_prompt: Some("Türkçe yanıtla".into()),
            settings: Some(json!({ "temperature": 0.7, "toolsEnabled": true })),
            ..Default::default()
        },
    )
    .unwrap();
    let u1 = append(&db, &conv.id, None, "user", "selam");
    let a1 = db
        .append_message(AppendMessage {
            conversation_id: conv.id.clone(),
            parent_id: Some(u1.id.clone()),
            role: "assistant".into(),
            variant: VariantInput {
                content: "```rust\nfn main() {}\n```".into(),
                reasoning: Some("düşünce".into()),
                tool_calls: Some(json!([{ "id": "t1", "name": "ls", "arguments": "{\"p\":\"```\"}", "status": "done", "result": "a.txt" }])),
                stats: Some(json!({ "predictedN": 5 })),
                ..Default::default()
            },
            id: Some("client-msg-1".into()),
            variant_id: Some("client-var-1".into()),
            any_parent_variant: false,
        })
        .unwrap();
    assert_eq!(a1.id, "client-msg-1");
    db.add_variant(&a1.id, &text("ikinci"), None).unwrap();
    db.edit_user_message(&u1.id, "merhaba", None).unwrap();
    db.select_branch(&u1.id).unwrap();
    db.select_variant(&a1.id, "client-var-1").unwrap();

    let export = db.export_json(&conv.id).unwrap();
    assert_eq!(export.schema, EXPORT_SCHEMA);
    assert_eq!(export.messages.len(), 3);
    let json_text = serde_json::to_string_pretty(&export).unwrap();

    let imported = db.import_json_str(&json_text).unwrap();
    assert_ne!(imported.id, conv.id);
    assert_eq!(imported.title, "Dışa aktar");
    assert_eq!(imported.created_at, export.conversation.created_at);
    assert_eq!(imported.system_prompt, "Türkçe yanıtla");
    assert_eq!(imported.settings["toolsEnabled"], json!(true));

    let orig = db.get_conversation(&conv.id).unwrap();
    let copy = db.get_conversation(&imported.id).unwrap();
    assert_eq!(contents(&orig), contents(&copy));
    assert_eq!(copy.messages[0].sibling_ids.len(), 2);
    assert_eq!(copy.messages[1].variant_ids.len(), 2);
    assert_eq!(
        orig.messages
            .iter()
            .map(|m| m.created_at)
            .collect::<Vec<_>>(),
        copy.messages
            .iter()
            .map(|m| m.created_at)
            .collect::<Vec<_>>()
    );

    // Re-exporting the import yields the same structure (ids differ).
    let again = db.export_json(&imported.id).unwrap();
    let strip = |e: &ConversationExport| -> Vec<(String, usize, Vec<String>)> {
        let mut v: Vec<_> = e
            .messages
            .iter()
            .map(|m| {
                (
                    m.role.clone(),
                    m.variants.len(),
                    m.variants.iter().map(|x| x.content.clone()).collect(),
                )
            })
            .collect();
        v.sort();
        v
    };
    assert_eq!(strip(&export), strip(&again));

    // Markdown export of the current path.
    let md = db
        .export_markdown(&conv.id, &MarkdownLabels::default())
        .unwrap();
    assert!(md.starts_with("# Dışa aktar"));
    assert!(md.contains("Türkçe yanıtla"));
    assert!(md.contains("### User"));
    assert!(md.contains("<summary>Reasoning</summary>"));
    assert!(md.contains("````")); // fence longer than the ``` inside the tool arguments

    assert!(md.contains("````\n\n**Result:**\n\n```\na.txt\n```"));
    let labels = MarkdownLabels {
        result: "Sonuç".into(),
        ..Default::default()
    };
    let localized = db.export_markdown(&conv.id, &labels).unwrap();
    assert!(localized.contains("**Sonuç:**\n\n```\na.txt\n```"));

    // Validation.
    let key_of = |e: StateError| {
        let text = e.to_string();
        crate::error_key::parse(&text).map(|(k, _)| k.to_string())
    };
    let e = db.import_json_str("{\"schema\":\"other\"}").unwrap_err();
    assert_eq!(key_of(e).as_deref(), Some("export.unsupported_format"));
    let e = db.import_json_str("not json").unwrap_err();
    assert_eq!(key_of(e).as_deref(), Some("json.invalid"));
    let mut broken = export.clone();
    broken.messages[0].parent_id = Some("missing".into());
    assert!(db.import_json(&broken).is_err());
    let mut cyclic = export.clone();
    let first = cyclic.messages[0].id.clone();
    let second = cyclic.messages[1].id.clone();
    cyclic.messages[0].parent_id = Some(second);
    cyclic.messages[1].parent_id = Some(first);
    let e = db.import_json(&cyclic).unwrap_err();
    assert_eq!(key_of(e).as_deref(), Some("export.cycle"));
    let mut dup_ids = export.clone();
    dup_ids.messages[1].id = dup_ids.messages[0].id.clone();
    let e = db.import_json(&dup_ids).unwrap_err();
    assert_eq!(key_of(e).as_deref(), Some("export.bad_message_id"));
}

#[test]
fn legacy_import_runs_once() {
    let (_d, db) = open_temp();
    let import = LinearImport {
        conversation: NewConversation {
            title: Some("Eski sohbet".into()),
            title_source: Some("fallback".into()),
            system_prompt: Some("eski sistem".into()),
            ..Default::default()
        },
        messages: vec![
            LinearMessage {
                role: "user".into(),
                variant: text("eski soru"),
            },
            LinearMessage {
                role: "assistant".into(),
                variant: text("eski cevap"),
            },
        ],
    };
    let id = db.import_legacy_playground(&import).unwrap().unwrap();
    let view = db.get_conversation(&id).unwrap();
    assert_eq!(contents(&view), ["eski soru", "eski cevap"]);
    assert_eq!(view.conversation.title_source, "fallback");
    assert_eq!(db.import_legacy_playground(&import).unwrap(), None);
    assert_eq!(
        db.list_conversations(&ListQuery::default()).unwrap().len(),
        1
    );
}

#[test]
fn playground_state_migration_is_verified_and_idempotent() {
    let (_d, db) = open_temp();
    let import = PlaygroundStateImport {
        conversation: Some(LinearImport {
            conversation: NewConversation {
                title: Some("Tarayıcı sohbeti".into()),
                ..Default::default()
            },
            messages: vec![LinearMessage {
                role: "user".into(),
                variant: text("kaybolmamalı"),
            }],
        }),
        values: vec![
            PlaygroundStateValue {
                key: "harness_playground_settings_v2".into(),
                value: r#"{"model":"qwen"}"#.into(),
            },
            PlaygroundStateValue {
                key: "harness_playground_compare_v1".into(),
                value: r#"{"enabled":true}"#.into(),
            },
        ],
    };

    let result = db.import_playground_state(&import).unwrap();
    assert!(result.conversation_verified);
    assert_eq!(result.verified_keys.len(), 2);
    assert_eq!(
        db.get_flag("harness_playground_settings_v2").unwrap(),
        Some(r#"{"model":"qwen"}"#.into())
    );
    assert_eq!(
        contents(
            &db.get_conversation(&db.list_conversations(&ListQuery::default()).unwrap()[0].id)
                .unwrap()
        ),
        ["kaybolmamalı"]
    );

    let again = db.import_playground_state(&import).unwrap();
    assert_eq!(again, result);
    assert_eq!(
        db.list_conversations(&ListQuery::default()).unwrap().len(),
        1
    );

    let conflict = PlaygroundStateImport {
        conversation: import.conversation,
        values: vec![PlaygroundStateValue {
            key: "harness_playground_settings_v2".into(),
            value: r#"{"model":"other"}"#.into(),
        }],
    };
    let result = db.import_playground_state(&conflict).unwrap();
    assert!(result.conversation_verified);
    assert!(result.verified_keys.is_empty());
    assert_eq!(
        db.get_flag("harness_playground_settings_v2").unwrap(),
        Some(r#"{"model":"qwen"}"#.into())
    );
}

#[test]
fn playground_state_migration_rolls_back_everything_on_failure() {
    let (_d, db) = open_temp();
    let broken = PlaygroundStateImport {
        conversation: Some(LinearImport {
            conversation: NewConversation::default(),
            messages: vec![LinearMessage {
                role: "invalid".into(),
                variant: text("x"),
            }],
        }),
        values: vec![PlaygroundStateValue {
            key: "harness_playground_settings_v2".into(),
            value: "must-not-stick".into(),
        }],
    };
    assert!(db.import_playground_state(&broken).is_err());
    assert_eq!(db.get_flag("harness_playground_settings_v2").unwrap(), None);
    assert_eq!(
        db.list_conversations(&ListQuery::default()).unwrap().len(),
        0
    );

    let fixed = PlaygroundStateImport {
        conversation: Some(LinearImport {
            conversation: NewConversation::default(),
            messages: vec![LinearMessage {
                role: "user".into(),
                variant: text("retry works"),
            }],
        }),
        values: broken.values,
    };
    let result = db.import_playground_state(&fixed).unwrap();
    assert!(result.conversation_verified);
    assert_eq!(result.verified_keys, ["harness_playground_settings_v2"]);
}

#[test]
fn rejects_bad_input() {
    let (_d, db) = open_temp();
    let conv = new_conv(&db, "x");
    let other = new_conv(&db, "y");
    let m = append(&db, &other.id, None, "user", "başka");
    // Parent from another conversation.
    assert!(db
        .append_message(AppendMessage {
            conversation_id: conv.id.clone(),
            parent_id: Some(m.id.clone()),
            role: "assistant".into(),
            variant: text("x"),
            id: None,
            variant_id: None,
            any_parent_variant: false,
        })
        .is_err());
    // Unknown role, bad client id.
    assert!(db
        .append_message(AppendMessage {
            conversation_id: conv.id.clone(),
            parent_id: None,
            role: "robot".into(),
            variant: text("x"),
            id: None,
            variant_id: None,
            any_parent_variant: false,
        })
        .is_err());
    assert!(db
        .append_message(AppendMessage {
            conversation_id: conv.id.clone(),
            parent_id: None,
            role: "user".into(),
            variant: text("x"),
            id: Some("../etc".into()),
            variant_id: None,
            any_parent_variant: false,
        })
        .is_err());
    // Variant of another message.
    let u = append(&db, &conv.id, None, "user", "q");
    assert!(db.select_variant(&u.id, &m.variant.id).is_err());
    assert!(db.set_title(&conv.id, "t", "bogus", None).is_err());
    assert!(db.create_folder("   ", None).is_err());
}

fn side(content: &str, group: &str, side: &str, model: &str) -> VariantInput {
    VariantInput {
        content: content.to_string(),
        model_alias: Some(model.to_string()),
        meta: Some(json!({
            "compare": { "group": group, "side": side, "model": model, "preset_name": format!("P-{side}") }
        })),
        ..Default::default()
    }
}

fn shown_sides(view: &ConversationView) -> Vec<Option<String>> {
    view.messages
        .iter()
        .map(|m| compare_of(m.variant.meta.as_ref()).map(|t| t.side))
        .collect()
}

#[test]
fn v3_database_upgrades_to_v4_and_keeps_variants() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("state.db");
    {
        // A file written by the previous release (schema v3) with one stored message.
        let conn = rusqlite::Connection::open(&path).unwrap();
        for sql in &schema::MIGRATIONS[..3] {
            conn.execute_batch(sql).unwrap();
        }
        conn.pragma_update(None, "user_version", 3).unwrap();
        conn.execute_batch(
            "INSERT INTO conversations (id, title, created_at, updated_at) VALUES ('c1', 'eski', 1, 1);
             INSERT INTO messages (id, conversation_id, role, created_at, active_variant_id)
                 VALUES ('m1', 'c1', 'user', 2, 'v1');
             INSERT INTO message_variants (id, message_id, content, created_at)
                 VALUES ('v1', 'm1', 'eski içerik', 2);",
        )
        .unwrap();
    }
    let db = StateDb::open(&path).unwrap();
    const { assert!(SCHEMA_VERSION >= 4) };
    assert_eq!(db.user_version().unwrap(), SCHEMA_VERSION);
    let view = db.get_conversation("c1").unwrap();
    assert_eq!(contents(&view), vec!["eski içerik"]);
    assert_eq!(view.messages[0].variant.meta, None);
    assert!(view.messages[0].compare.is_empty());
    // The new column is writable; the search index still follows the content.
    let mut v = text("yeni içerik");
    v.meta = Some(json!({ "compare": { "group": "g", "side": "a" } }));
    db.update_variant("v1", &v).unwrap();
    let view = db.get_conversation("c1").unwrap();
    assert_eq!(
        compare_of(view.messages[0].variant.meta.as_ref()),
        Some(CompareTag {
            group: "g".into(),
            side: "a".into()
        })
    );
    let hits = db
        .list_conversations(&ListQuery {
            query: Some("yeni".into()),
            ..Default::default()
        })
        .unwrap();
    assert_eq!(hits.len(), 1);
    // Re-opening an up-to-date file changes nothing.
    drop(db);
    let db = StateDb::open(&path).unwrap();
    assert_eq!(db.user_version().unwrap(), SCHEMA_VERSION);
}

#[test]
fn v4_database_upgrades_to_v5_with_document_tables() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("state.db");
    {
        // A file written by the previous release (schema v4) with a conversation.
        let conn = rusqlite::Connection::open(&path).unwrap();
        for sql in &schema::MIGRATIONS[..4] {
            conn.execute_batch(sql).unwrap();
        }
        conn.pragma_update(None, "user_version", 4).unwrap();
        conn.execute_batch(
            "INSERT INTO conversations (id, title, created_at, updated_at) VALUES ('c1', 'eski', 1, 1);
             INSERT INTO messages (id, conversation_id, role, created_at, active_variant_id)
                 VALUES ('m1', 'c1', 'user', 2, 'v1');
             INSERT INTO message_variants (id, message_id, content, created_at, meta_json)
                 VALUES ('v1', 'm1', 'eski içerik', 2, '{\"k\":1}');",
        )
        .unwrap();
    }
    let db = StateDb::open(&path).unwrap();
    const { assert!(SCHEMA_VERSION >= 5) };
    assert_eq!(db.user_version().unwrap(), SCHEMA_VERSION);
    let view = db.get_conversation("c1").unwrap();
    assert_eq!(contents(&view), vec!["eski içerik"]);
    assert_eq!(view.messages[0].variant.meta, Some(json!({"k": 1})));
    // The old conversation takes documents; chunks reach the keyword index.
    let doc = db.rag_create_document("c1", "not.md", None).unwrap();
    assert_eq!(
        (doc.status.as_str(), doc.mode.as_str()),
        ("pending", "auto")
    );
    let chunk = crate::rag::chunk::Chunk {
        text: "Kalıcı belge içeriği".into(),
        page_start: None,
        page_end: None,
        heading: Some("Başlık".into()),
        ..Default::default()
    };
    db.rag_store_extraction(
        &doc.id,
        "markdown",
        21,
        "h",
        None,
        "Kalıcı belge içeriği",
        &[chunk],
        10,
    )
    .unwrap();
    let hits = db
        .rag_keyword(std::slice::from_ref(&doc.id), "\"icerigi\"", 5)
        .unwrap();
    assert_eq!(
        hits.len(),
        1,
        "diacritics are folded like the conversation search"
    );
    assert!(db.rag_create_document("missing", "x", None).is_err());
    drop(db);
    let db = StateDb::open(&path).unwrap();
    assert_eq!(db.user_version().unwrap(), SCHEMA_VERSION);
    assert_eq!(db.rag_documents("c1").unwrap().len(), 1);
}

#[test]
fn split_turns_store_both_sides_and_adopt_one() {
    let (_d, db) = open_temp();
    let conv = new_conv(&db, "karşılaştırma");
    let u1 = append(&db, &conv.id, None, "user", "soru bir");
    // Split turn 1: side a is the message, side b a variant of it; a stays shown.
    let m1 = db
        .append_message(AppendMessage {
            conversation_id: conv.id.clone(),
            parent_id: Some(u1.id.clone()),
            role: "assistant".into(),
            variant: side("a1", "g1", "a", "model-a"),
            id: None,
            variant_id: None,
            any_parent_variant: false,
        })
        .unwrap();
    let b1 = db
        .add_variant(&m1.id, &side("b1", "g1", "b", "model-b"), None)
        .unwrap();
    db.select_variant(&m1.id, &m1.variant.id).unwrap();

    // The continuation belongs to both sides.
    let u2 = db
        .append_message(AppendMessage {
            conversation_id: conv.id.clone(),
            parent_id: Some(m1.id.clone()),
            role: "user".into(),
            variant: text("soru iki"),
            id: None,
            variant_id: None,
            any_parent_variant: true,
        })
        .unwrap();
    assert_eq!(u2.parent_variant_id, None);
    let m2 = db
        .append_message(AppendMessage {
            conversation_id: conv.id.clone(),
            parent_id: Some(u2.id.clone()),
            role: "assistant".into(),
            variant: side("a2", "g2", "a", "model-a"),
            id: None,
            variant_id: None,
            any_parent_variant: false,
        })
        .unwrap();
    db.add_variant(&m2.id, &side("b2", "g2", "b", "model-b"), None)
        .unwrap();
    db.select_variant(&m2.id, &m2.variant.id).unwrap();

    let view = db.get_conversation(&conv.id).unwrap();
    assert_eq!(contents(&view), vec!["soru bir", "a1", "soru iki", "a2"]);
    // Each split message carries its group in side order; a normal message none.
    assert!(view.messages[0].compare.is_empty());
    let sides: Vec<&str> = view.messages[1]
        .compare
        .iter()
        .map(|v| v.content.as_str())
        .collect();
    assert_eq!(sides, vec!["a1", "b1"]);
    assert_eq!(view.messages[3].compare.len(), 2);
    assert_eq!(view.messages[1].variant_ids.len(), 2);

    // Showing side b of turn 1 keeps the (shared) continuation on the path.
    let view = db.select_variant(&m1.id, &b1.variant.id).unwrap();
    assert_eq!(contents(&view), vec!["soru bir", "b1", "soru iki", "a2"]);

    // "Continue with B": every split message switches to its b side, the leaf stays.
    let view = db.adopt_compare_side(&conv.id, "b").unwrap();
    assert_eq!(contents(&view), vec!["soru bir", "b1", "soru iki", "b2"]);
    assert_eq!(
        shown_sides(&view),
        vec![None, Some("b".into()), None, Some("b".into())]
    );
    assert_eq!(
        view.conversation.current_leaf_id.as_deref(),
        Some(m2.id.as_str())
    );
    // A normal message after that continues side b.
    let b2_id = view.messages[3].variant.id.clone();
    let u3 = append(&db, &conv.id, Some(&m2.id), "user", "devam");
    assert_eq!(u3.parent_variant_id.as_deref(), Some(b2_id.as_str()));
    // Adopting a side that does not exist changes nothing.
    let view = db.adopt_compare_side(&conv.id, "z").unwrap();
    assert_eq!(
        contents(&view),
        vec!["soru bir", "b1", "soru iki", "b2", "devam"]
    );

    // Metadata that is not an object is not stored.
    let odd = db
        .add_variant(
            &m2.id,
            &VariantInput {
                content: "x".into(),
                meta: Some(json!("not an object")),
                ..Default::default()
            },
            None,
        )
        .unwrap();
    assert_eq!(odd.variant.meta, None);
    assert!(odd.compare.is_empty());
    db.select_variant(&m2.id, &b2_id).unwrap();
    // A content update without metadata keeps the side.
    db.update_variant(&b2_id, &text("b2")).unwrap();
    let view = db.get_conversation(&conv.id).unwrap();
    assert_eq!(shown_sides(&view)[3].as_deref(), Some("b"));
    assert_eq!(view.messages[3].compare.len(), 2);

    // JSON export keeps both sides with their metadata; import restores the groups.
    let export = db.export_json(&conv.id).unwrap();
    let m1_export = export.messages.iter().find(|m| m.id == m1.id).unwrap();
    assert_eq!(m1_export.variants.len(), 2);
    assert!(m1_export
        .variants
        .iter()
        .all(|v| compare_of(v.meta.as_ref()).is_some()));
    let text_json = serde_json::to_string(&export).unwrap();
    let imported = db.import_json_str(&text_json).unwrap();
    let iv = db.get_conversation(&imported.id).unwrap();
    assert_eq!(
        contents(&iv),
        vec!["soru bir", "b1", "soru iki", "b2", "devam"]
    );
    assert_eq!(iv.messages[1].compare.len(), 2);
    assert_eq!(iv.messages[3].compare.len(), 2);

    // Markdown shows both sides of a split turn under their own headings.
    let md = db
        .export_markdown(&conv.id, &MarkdownLabels::default())
        .unwrap();
    assert!(md.contains("#### A · `model-a` · P-a\n\na1"), "{md}");
    assert!(md.contains("#### B · `model-b` · P-b\n\nb1"), "{md}");
    assert!(md.contains("#### B · `model-b` · P-b\n\nb2"), "{md}");
    assert!(md.contains("devam"));
}

#[test]
fn v5_database_upgrades_with_stored_responses_and_events() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("state.db");
    {
        // A file written by the previous release (schema v5) with a conversation.
        let conn = rusqlite::Connection::open(&path).unwrap();
        for sql in &schema::MIGRATIONS[..5] {
            conn.execute_batch(sql).unwrap();
        }
        conn.pragma_update(None, "user_version", 5).unwrap();
        conn.execute_batch(
            "INSERT INTO conversations (id, title, created_at, updated_at) VALUES ('c1', 'eski', 1, 1);",
        )
        .unwrap();
    }
    let db = StateDb::open(&path).unwrap();
    const { assert!(SCHEMA_VERSION >= 6) };
    assert_eq!(db.user_version().unwrap(), SCHEMA_VERSION);
    assert_eq!(
        db.get_conversation("c1").unwrap().conversation.title,
        "eski"
    );
    assert_eq!(
        db.responses_stats().unwrap(),
        StoredResponsesStats::default()
    );
    let retention = crate::gateway::responses::ResponsesSettings::default().retention();
    db.put_response(
        &NewStoredResponse {
            id: "resp_1".into(),
            scope: "anonymous".into(),
            previous_id: None,
            model: "m".into(),
            created_at: now_ms(),
            input: vec![json!({"role": "user", "content": "hi"})],
            response: json!({"id": "resp_1", "output": []}),
        },
        &retention,
    )
    .unwrap();
    assert!(db
        .get_response("anonymous", "resp_1", &retention)
        .unwrap()
        .is_some());
    let event = db
        .append_response_event(
            "anonymous",
            "resp_1",
            json!({"type":"response.completed"}),
            &retention,
        )
        .unwrap()
        .unwrap();
    assert_eq!(event.sequence_number, 0);
}

#[test]
fn regenerating_a_split_turn_adds_a_group_next_to_the_first() {
    let (_d, db) = open_temp();
    let conv = new_conv(&db, "yeniden uret");
    let u = append(&db, &conv.id, None, "user", "soru");
    let m = db
        .append_message(AppendMessage {
            conversation_id: conv.id.clone(),
            parent_id: Some(u.id.clone()),
            role: "assistant".into(),
            variant: side("a1", "g1", "a", "model-a"),
            id: None,
            variant_id: None,
            any_parent_variant: false,
        })
        .unwrap();
    db.add_variant(&m.id, &side("b1", "g1", "b", "model-b"), None)
        .unwrap();
    db.select_variant(&m.id, &m.variant.id).unwrap();
    // Regenerate: a second pair, side a shown.
    let a2 = db
        .add_variant(&m.id, &side("a2", "g2", "a", "model-a"), None)
        .unwrap();
    let b2 = db
        .add_variant(&m.id, &side("b2", "g2", "b", "model-b"), None)
        .unwrap();
    let view = db.select_variant(&m.id, &a2.variant.id).unwrap();
    let shown = &view.messages[1];
    let group: Vec<&str> = shown.compare.iter().map(|v| v.content.as_str()).collect();
    assert_eq!(group, vec!["a2", "b2"]);
    // One representative (side a) per group, oldest first; the shown group is the second.
    assert_eq!(
        shown.compare_groups,
        vec![m.variant.id.clone(), a2.variant.id.clone()]
    );
    let view = db.select_variant(&m.id, &b2.variant.id).unwrap();
    assert_eq!(view.messages[1].compare.len(), 2);
    assert_eq!(view.messages[1].compare_groups.len(), 2);
    // A plain message has no groups.
    assert!(view.messages[0].compare_groups.is_empty());
}

#[test]
fn rewind_removes_the_message_and_what_follows_from_the_path() {
    let (_d, db) = open_temp();
    let conv = new_conv(&db, "geri sar");
    let u1 = append(&db, &conv.id, None, "user", "bir");
    let a1 = append(&db, &conv.id, Some(&u1.id), "assistant", "cevap bir");
    let u2 = append(&db, &conv.id, Some(&a1.id), "user", "iki");
    let a2 = append(&db, &conv.id, Some(&u2.id), "assistant", "cevap iki");
    let u3 = append(&db, &conv.id, Some(&a2.id), "user", "uc");
    append(&db, &conv.id, Some(&u3.id), "assistant", "cevap uc");

    let view = db.rewind_before(&u2.id).unwrap();
    assert_eq!(contents(&view), vec!["bir", "cevap bir"]);
    assert_eq!(
        view.conversation.current_leaf_id.as_deref(),
        Some(a1.id.as_str())
    );
    // Persisted: reopening shows the same path.
    assert_eq!(
        contents(&db.open_conversation(&conv.id).unwrap()),
        vec!["bir", "cevap bir"]
    );
    // Nothing was deleted: sending from here makes a branch and the old one is recoverable.
    let u2b = append(&db, &conv.id, Some(&a1.id), "user", "iki (duzeltilmis)");
    let view = db.get_conversation(&conv.id).unwrap();
    assert_eq!(
        contents(&view),
        vec!["bir", "cevap bir", "iki (duzeltilmis)"]
    );
    assert_eq!(
        view.messages[2].sibling_ids,
        vec![u2.id.clone(), u2b.id.clone()]
    );
    let view = db.select_branch(&u2.id).unwrap();
    assert_eq!(
        contents(&view),
        vec!["bir", "cevap bir", "iki", "cevap iki", "uc", "cevap uc"]
    );

    // Only user messages can be rewound.
    assert!(db.rewind_before(&a1.id).is_err());
}

#[test]
fn rewinding_the_first_message_leaves_an_empty_path_until_the_next_message() {
    let (_d, db) = open_temp();
    let conv = new_conv(&db, "bastan");
    let u1 = append(&db, &conv.id, None, "user", "ilk");
    append(&db, &conv.id, Some(&u1.id), "assistant", "cevap");
    let view = db.rewind_before(&u1.id).unwrap();
    assert!(view.messages.is_empty());
    assert!(view.conversation.current_leaf_id.is_none());
    // Stays empty after reopening (the old branch is not shown again by itself).
    assert!(db.open_conversation(&conv.id).unwrap().messages.is_empty());
    // A copy keeps the rewound state.
    let dup = db.duplicate_conversation(&conv.id, None).unwrap();
    assert!(db.get_conversation(&dup.id).unwrap().messages.is_empty());
    // The new first message is a sibling of the old one.
    let u1b = append(&db, &conv.id, None, "user", "ilk (yeni)");
    let view = db.get_conversation(&conv.id).unwrap();
    assert_eq!(contents(&view), vec!["ilk (yeni)"]);
    assert_eq!(
        view.messages[0].sibling_ids,
        vec![u1.id.clone(), u1b.id.clone()]
    );
    let view = db.select_branch(&u1.id).unwrap();
    assert_eq!(contents(&view), vec!["ilk", "cevap"]);
}

#[test]
fn rewind_restores_the_parent_variant_the_message_followed() {
    let (_d, db) = open_temp();
    let conv = new_conv(&db, "varyant");
    let u1 = append(&db, &conv.id, None, "user", "soru");
    let a = append(&db, &conv.id, Some(&u1.id), "assistant", "cevap 1");
    let v2 = db.add_variant(&a.id, &text("cevap 2"), None).unwrap();
    db.select_variant(&a.id, &a.variant.id).unwrap();
    // The follow-up was written under variant 1; the answer was then regenerated (variant 2
    // active, the path left the follow-up).
    let u2 = append(&db, &conv.id, Some(&a.id), "user", "devam");
    let view = db.select_variant(&a.id, &v2.variant.id).unwrap();
    assert_eq!(contents(&view), vec!["soru", "cevap 2"]);
    let view = db.rewind_before(&u2.id).unwrap();
    assert_eq!(contents(&view), vec!["soru", "cevap 1"]);
}

/// u1 a1 u2 a2 u3 a3 as one path; returns the ids in that order.
fn six_messages(db: &StateDb, conv: &str) -> Vec<PathMessage> {
    let mut out: Vec<PathMessage> = Vec::new();
    for (i, (role, content)) in [
        ("user", "soru 1"),
        ("assistant", "cevap 1"),
        ("user", "soru 2"),
        ("assistant", "cevap 2"),
        ("user", "soru 3"),
        ("assistant", "cevap 3"),
    ]
    .into_iter()
    .enumerate()
    {
        let parent = if i == 0 {
            None
        } else {
            Some(out[i - 1].id.clone())
        };
        out.push(append(db, conv, parent.as_deref(), role, content));
    }
    out
}

#[test]
fn a_regenerated_variant_sorts_after_a_first_variant_stamped_ahead_of_the_clock() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("state.db");
    let db = StateDb::open(&path).unwrap();
    let conv = new_conv(&db, "Saat ileride");
    let u = append(&db, &conv.id, None, "user", "soru");
    let a = append(&db, &conv.id, Some(&u.id), "assistant", "cevap");
    // Messages appended faster than the clock ticks are stamped ahead of it; force that.
    {
        let conn = rusqlite::Connection::open(&path).unwrap();
        conn.execute(
            "UPDATE message_variants SET created_at = created_at + 3600000 WHERE id = ?1",
            [&a.variant.id],
        )
        .unwrap();
    }
    let b = db.add_variant(&a.id, &text("cevap b"), None).unwrap();
    assert_eq!(b.variant_ids, [a.variant.id.clone(), b.variant.id.clone()]);
    assert_eq!(b.variant_index, 1);
    let view = db.select_variant(&a.id, &a.variant.id).unwrap();
    assert_eq!(view.messages[1].variant_index, 0);
}

#[test]
fn regenerating_a_message_in_the_middle_keeps_each_variants_continuation() {
    let (_d, db) = open_temp();
    let conv = new_conv(&db, "Ortadan yeniden üret");
    let m = six_messages(&db, &conv.id);
    let (a1, a2) = (&m[1], &m[3]);

    // Regenerate the first answer: the path ends there, the old continuation is not on it.
    let r1 = db.add_variant(&a1.id, &text("cevap 1b"), None).unwrap();
    let view = db.get_conversation(&conv.id).unwrap();
    assert_eq!(contents(&view), ["soru 1", "cevap 1b"]);
    assert_eq!(r1.variant_ids.len(), 2);

    // The new variant gets its own continuation.
    let u2b = append(&db, &conv.id, Some(&a1.id), "user", "soru 2b");
    append(&db, &conv.id, Some(&u2b.id), "assistant", "cevap 2b");
    let view = db.get_conversation(&conv.id).unwrap();
    assert_eq!(
        contents(&view),
        ["soru 1", "cevap 1b", "soru 2b", "cevap 2b"]
    );

    // Variant 1 shows its own downstream branch, variant 2 shows the other one.
    let view = db.select_variant(&a1.id, &a1.variant.id).unwrap();
    assert_eq!(
        contents(&view),
        ["soru 1", "cevap 1", "soru 2", "cevap 2", "soru 3", "cevap 3"]
    );
    assert_eq!(
        (
            view.messages[1].variant_index,
            view.messages[1].variant_ids.len()
        ),
        (0, 2)
    );
    let view = db.select_variant(&a1.id, &r1.variant.id).unwrap();
    assert_eq!(
        contents(&view),
        ["soru 1", "cevap 1b", "soru 2b", "cevap 2b"]
    );

    // A third variant, then regenerate a message deeper down (on the first variant's branch).
    let r1c = db.add_variant(&a1.id, &text("cevap 1c"), None).unwrap();
    assert_eq!(r1c.variant_ids.len(), 3);
    assert_eq!(r1c.variant_index, 2);
    db.select_variant(&a1.id, &a1.variant.id).unwrap();
    let r2 = db.add_variant(&a2.id, &text("cevap 2'"), None).unwrap();
    let view = db.get_conversation(&conv.id).unwrap();
    assert_eq!(contents(&view), ["soru 1", "cevap 1", "soru 2", "cevap 2'"]);
    // Question 3 and its answer hang below the first variant of answer 2.
    let view = db.select_variant(&a2.id, &a2.variant.id).unwrap();
    assert_eq!(view.messages.len(), 6);
    assert_eq!(view.messages[3].variant_index, 0);
    assert_eq!(view.messages[3].variant_ids.len(), 2);
    let view = db.select_variant(&a2.id, &r2.variant.id).unwrap();
    assert_eq!(view.messages.len(), 4);
    // The other branch of the first answer is untouched.
    let view = db.select_variant(&a1.id, &r1.variant.id).unwrap();
    assert_eq!(
        contents(&view),
        ["soru 1", "cevap 1b", "soru 2b", "cevap 2b"]
    );
}

#[test]
fn editing_a_user_message_in_the_middle_branches_from_there() {
    let (_d, db) = open_temp();
    let conv = new_conv(&db, "Ortadan düzenle");
    let m = six_messages(&db, &conv.id);
    let edited = db
        .edit_user_message(&m[2].id, "soru 2 (düzenli)", None)
        .unwrap();
    append(&db, &conv.id, Some(&edited.id), "assistant", "yeni cevap");
    let view = db.get_conversation(&conv.id).unwrap();
    assert_eq!(
        contents(&view),
        ["soru 1", "cevap 1", "soru 2 (düzenli)", "yeni cevap"]
    );
    assert_eq!(
        view.messages[2].sibling_ids,
        vec![m[2].id.clone(), edited.id.clone()]
    );
    // Back to the original question: its whole continuation is there.
    let view = db.select_branch(&m[2].id).unwrap();
    assert_eq!(view.messages.len(), 6);
    assert_eq!(view.messages[2].sibling_index, 0);
}

#[test]
fn fork_from_any_message_uses_the_variant_shown_there() {
    let (_d, db) = open_temp();
    let conv = new_conv(&db, "Çatal");
    let m = six_messages(&db, &conv.id);
    let a1 = &m[1];
    // A second variant of answer 1 becomes active; the original keeps its continuation.
    db.add_variant(&a1.id, &text("cevap 1b"), None).unwrap();
    // Reaching the old continuation through a deeper message shows variant 1 again.
    let view = db.select_branch(&m[4].id).unwrap();
    assert_eq!(view.messages[1].variant.content, "cevap 1");
    let fork = db.fork_conversation(&a1.id, None).unwrap();
    assert_eq!(
        contents(&db.get_conversation(&fork.id).unwrap()),
        ["soru 1", "cevap 1"]
    );

    // Fork from a user message in the middle: history up to and including it.
    let fork = db.fork_conversation(&m[2].id, None).unwrap();
    let view = db.get_conversation(&fork.id).unwrap();
    assert_eq!(contents(&view), ["soru 1", "cevap 1", "soru 2"]);
    assert_eq!(
        fork.forked_from_message_id.as_deref(),
        Some(m[2].id.as_str())
    );
    // The source is unchanged.
    assert_eq!(db.get_conversation(&conv.id).unwrap().messages.len(), 6);
}

#[test]
fn regenerating_before_a_compaction_marker_leaves_it_on_the_old_branch() {
    let (_d, db) = open_temp();
    let conv = new_conv(&db, "Sıkıştırma");
    let m = six_messages(&db, &conv.id);
    // A summary marker after answer 3, then one more question.
    let marker = db
        .append_message(AppendMessage {
            conversation_id: conv.id.clone(),
            parent_id: Some(m[5].id.clone()),
            role: "system".into(),
            variant: VariantInput {
                content: "özet".into(),
                meta: Some(json!({ "compaction": { "covered": 4 } })),
                ..Default::default()
            },
            id: None,
            variant_id: None,
            any_parent_variant: false,
        })
        .unwrap();
    append(&db, &conv.id, Some(&marker.id), "user", "soru 4");

    // Regenerating an answer before the marker: the new path is the plain history.
    let a2 = &m[3];
    db.add_variant(&a2.id, &text("cevap 2b"), None).unwrap();
    let view = db.get_conversation(&conv.id).unwrap();
    assert_eq!(contents(&view), ["soru 1", "cevap 1", "soru 2", "cevap 2b"]);
    assert!(view.messages.iter().all(|p| p.role != "system"));

    // Switching back restores the marker with the history it summarized.
    let view = db.select_variant(&a2.id, &a2.variant.id).unwrap();
    assert_eq!(view.messages.len(), 8);
    assert_eq!(view.messages[6].role, "system");
    assert_eq!(view.messages[7].variant.content, "soru 4");

    // Editing the question right after the marker keeps the marker in the history.
    let edited = db
        .edit_user_message(&view.messages[7].id, "soru 4b", None)
        .unwrap();
    assert_eq!(edited.parent_id.as_deref(), Some(marker.id.as_str()));
    let view = db.get_conversation(&conv.id).unwrap();
    assert_eq!(view.messages.len(), 8);
    assert_eq!(view.messages[6].role, "system");
}

#[test]
fn edited_user_message_keeps_its_pictures() {
    let (_d, db) = open_temp();
    let conv = new_conv(&db, "Görsel");
    let image = serde_json::json!({ "name": "a.png", "url": "data:image/png;base64,AAAA" });
    let u1 = db
        .append_message(AppendMessage {
            conversation_id: conv.id.clone(),
            parent_id: None,
            role: "user".into(),
            variant: VariantInput {
                content: "ne görüyorsun?".into(),
                meta: Some(serde_json::json!({ "images": [image.clone()], "other": 1 })),
                ..Default::default()
            },
            id: None,
            variant_id: None,
            any_parent_variant: false,
        })
        .unwrap();
    append(&db, &conv.id, Some(&u1.id), "assistant", "bir daire");

    let edited = db
        .edit_user_message(&u1.id, "kaç şekil var?", None)
        .unwrap();
    // Only the pictures move over; other client metadata stays with the old version.
    assert_eq!(
        edited.variant.meta,
        Some(serde_json::json!({ "images": [image] }))
    );

    // Editing the edited version keeps them too; a message without pictures gets no metadata.
    let again = db
        .edit_user_message(&edited.id, "kaç renk var?", None)
        .unwrap();
    assert!(again.variant.meta.is_some_and(|m| m["images"].is_array()));
    let a = append(&db, &conv.id, Some(&edited.id), "assistant", "iki");
    let u2 = append(&db, &conv.id, Some(&a.id), "user", "teşekkürler");
    let plain = db.edit_user_message(&u2.id, "sağ ol", None).unwrap();
    assert_eq!(plain.variant.meta, None);
}

#[test]
fn edited_user_message_keeps_its_audio() {
    let (_d, db) = open_temp();
    let conv = new_conv(&db, "Audio");
    let audio = serde_json::json!({ "name": "voice.wav", "format": "wav", "data": "UklGRg==" });
    let user = db
        .append_message(AppendMessage {
            conversation_id: conv.id.clone(),
            parent_id: None,
            role: "user".into(),
            variant: VariantInput {
                content: "Listen".into(),
                meta: Some(serde_json::json!({ "audio": [audio.clone()], "other": 1 })),
                ..Default::default()
            },
            id: None,
            variant_id: None,
            any_parent_variant: false,
        })
        .unwrap();
    let edited = db.edit_user_message(&user.id, "Transcribe", None).unwrap();
    assert_eq!(
        edited.variant.meta,
        Some(serde_json::json!({ "audio": [audio] }))
    );
    let again = db.edit_user_message(&edited.id, "Translate", None).unwrap();
    assert_eq!(again.variant.meta, edited.variant.meta);
}

#[test]
fn v7_database_upgrades_to_knowledge_bases_without_losing_data() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("state.db");
    {
        // A file written by the previous release (schema v7) with a conversation, a message
        // and a conversation document with chunks.
        let conn = rusqlite::Connection::open(&path).unwrap();
        for sql in &schema::MIGRATIONS[..7] {
            conn.execute_batch(sql).unwrap();
        }
        conn.pragma_update(None, "user_version", 7).unwrap();
        conn.execute_batch(
            "INSERT INTO conversations (id, title, created_at, updated_at) VALUES ('c1', 'eski sohbet', 1, 1);
             INSERT INTO rag_documents (id, conversation_id, name, status, created_at, updated_at)
             VALUES ('d1', 'c1', 'not.md', 'ready', 1, 1);",
        )
        .unwrap();
    }
    let db = StateDb::open(&path).unwrap();
    assert_eq!(db.user_version().unwrap(), SCHEMA_VERSION);
    assert_eq!(
        db.get_conversation("c1").unwrap().conversation.title,
        "eski sohbet"
    );
    assert_eq!(db.rag_documents("c1").unwrap().len(), 1);
    assert!(db.kb_list().unwrap().is_empty());
    // Opening again is a no-op, and the new tables work.
    drop(db);
    let db = StateDb::open(&path).unwrap();
    assert_eq!(db.user_version().unwrap(), SCHEMA_VERSION);
    let kb = db.kb_create("Notlar", None).unwrap();
    db.kb_attach("c1", &kb.id).unwrap();
    assert_eq!(db.kb_attached("c1").unwrap().len(), 1);
}

#[test]
fn knowledge_base_row_stays_out_of_lists_and_search() {
    let dir = tempfile::tempdir().unwrap();
    let db = StateDb::open(&dir.path().join("state.db")).unwrap();
    let kb = db.kb_create("Zebra Notes", None).unwrap();
    let chat = db
        .create_conversation(&NewConversation {
            title: Some("Sohbet".into()),
            ..Default::default()
        })
        .unwrap();
    let ids = |q: ListQuery| -> Vec<String> {
        db.list_conversations(&q)
            .unwrap()
            .into_iter()
            .map(|c| c.id)
            .collect()
    };
    assert_eq!(ids(ListQuery::default()), vec![chat.id.clone()]);
    // The title of the hidden row is in the search index, but never in the results.
    let found = ids(ListQuery {
        query: Some("Zebra".into()),
        ..Default::default()
    });
    assert!(found.is_empty(), "{found:?}");
    db.kb_delete(&kb.id).unwrap();
    assert!(db.kb_list().unwrap().is_empty());
}

#[test]
fn v7_database_upgrades_through_knowledge_bases_to_projects_without_losing_data() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("state.db");
    {
        // A file written by the release before both (schema v7).
        let conn = rusqlite::Connection::open(&path).unwrap();
        for sql in &schema::MIGRATIONS[..7] {
            conn.execute_batch(sql).unwrap();
        }
        conn.pragma_update(None, "user_version", 7).unwrap();
        conn.execute_batch(
            "INSERT INTO conversations (id, title, created_at, updated_at) VALUES ('c1', 'old', 1, 1);
             INSERT INTO rag_documents (id, conversation_id, name, status, created_at, updated_at)
             VALUES ('d1', 'c1', 'note.md', 'ready', 1, 1);",
        )
        .unwrap();
    }
    // The release in between (schema v8: knowledge bases) writes a base and an attachment.
    {
        let conn = rusqlite::Connection::open(&path).unwrap();
        conn.execute_batch(schema::MIGRATIONS[7]).unwrap();
        conn.pragma_update(None, "user_version", 8).unwrap();
        conn.execute_batch(
            "INSERT INTO conversations (id, title, created_at, updated_at) VALUES ('kb1', 'Knowledge base: Notes', 1, 1);
             INSERT INTO rag_kbs (id, name, created_at, updated_at) VALUES ('kb1', 'Notes', 1, 1);
             INSERT INTO rag_conversation_kbs (conversation_id, kb_id, created_at) VALUES ('c1', 'kb1', 1);",
        )
        .unwrap();
    }
    let db = StateDb::open(&path).unwrap();
    assert_eq!(db.user_version().unwrap(), SCHEMA_VERSION);
    assert_eq!(schema::MIGRATIONS.len(), 12);
    // v7 and v8 data is intact.
    assert_eq!(db.get_conversation("c1").unwrap().conversation.title, "old");
    assert_eq!(db.rag_documents("c1").unwrap().len(), 1);
    assert_eq!(db.kb_list().unwrap().len(), 1);
    assert_eq!(db.kb_attached("c1").unwrap().len(), 1);
    // v10 adds the OCR columns: nothing recognized, no OCR model.
    assert_eq!(db.kb_get("kb1").unwrap().ocr_model, None);
    assert!(db.rag_documents("c1").unwrap()[0].ocr_pages.is_empty());
    // The v9 tables work next to them.
    assert_eq!(db.conversation_project("c1").unwrap(), None);
    let project = db
        .create_project(&ProjectInput {
            name: "Report".into(),
            ..Default::default()
        })
        .unwrap();
    db.set_conversation_project("c1", Some(&project.id))
        .unwrap();
    let listed = db
        .list_conversations(&ListQuery {
            project_id: Some(project.id.clone()),
            ..Default::default()
        })
        .unwrap();
    assert_eq!(listed.len(), 1);
    // The hidden knowledge base row is not a chat.
    assert_eq!(
        db.list_conversations(&ListQuery::default()).unwrap().len(),
        1
    );
    // Deleting the project keeps the chat, without a project.
    db.delete_project(&project.id).unwrap();
    assert_eq!(db.conversation_project("c1").unwrap(), None);
    assert!(db.get_conversation("c1").is_ok());
}
