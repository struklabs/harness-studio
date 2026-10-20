use super::*;
use crate::state::{AppendMessage, ListQuery, NewConversation, ProjectSettings, VariantInput};
use std::io::Read;

fn setup() -> (tempfile::TempDir, StateDb, PathBuf, PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let db = StateDb::open(&dir.path().join("state.db")).unwrap();
    let folder = dir.path().join("project");
    let extra = dir.path().join("extra");
    std::fs::create_dir_all(&folder).unwrap();
    std::fs::create_dir_all(&extra).unwrap();
    (dir, db, folder, extra)
}

fn create(db: &StateDb, folder: &Path, extra: &Path) -> Project {
    let input = check_input(ProjectInput {
        name: " Launch ".into(),
        description: "Docs".into(),
        folder: Some(folder.to_string_lossy().into()),
        settings: ProjectSettings {
            allowed_folders: vec![
                extra.to_string_lossy().into(),
                extra.to_string_lossy().into(),
                String::new(),
            ],
            tools: vec!["write_file".into(), "write_file".into()],
            ..Default::default()
        },
    })
    .unwrap();
    db.create_project(&input).unwrap()
}

fn chat(db: &StateDb, project: Option<&str>, text: &str) -> String {
    let conv = db
        .create_conversation(&NewConversation {
            title: Some(text.into()),
            project_id: project.map(str::to_string),
            ..Default::default()
        })
        .unwrap();
    db.append_message(AppendMessage {
        conversation_id: conv.id.clone(),
        parent_id: None,
        role: "user".into(),
        variant: VariantInput {
            content: text.into(),
            ..Default::default()
        },
        id: None,
        variant_id: None,
        any_parent_variant: false,
    })
    .unwrap();
    conv.id
}

#[test]
fn folders_are_checked_and_normalized() {
    let (dir, db, folder, extra) = setup();
    let project = create(&db, &folder, &extra);
    assert_eq!(project.name, "Launch");
    assert_eq!(project.settings.allowed_folders.len(), 1);
    assert_eq!(project.settings.tools, ["write_file"]);
    assert_eq!(roots(&project).len(), 2);
    for bad in ["relative/dir", "/definitely/missing/folder"] {
        let input = ProjectInput {
            name: "x".into(),
            folder: Some(bad.into()),
            ..Default::default()
        };
        assert!(check_input(input).is_err(), "{bad}");
    }
    let file = dir.path().join("file.txt");
    std::fs::write(&file, "x").unwrap();
    assert!(check_input(ProjectInput {
        name: "x".into(),
        folder: Some(file.to_string_lossy().into()),
        ..Default::default()
    })
    .is_err());
    assert!(db
        .create_project(&ProjectInput {
            name: "  ".into(),
            ..Default::default()
        })
        .is_err());
}

#[test]
fn chats_are_filed_filtered_and_forked_within_the_project() {
    let (_dir, db, folder, extra) = setup();
    let project = create(&db, &folder, &extra);
    let inside = chat(&db, Some(&project.id), "launch plan");
    let outside = chat(&db, None, "launch notes");
    let only = |query: Option<&str>| {
        db.list_conversations(&ListQuery {
            project_id: Some(project.id.clone()),
            query: query.map(str::to_string),
            ..Default::default()
        })
        .unwrap()
        .into_iter()
        .map(|c| c.id)
        .collect::<Vec<_>>()
    };
    assert_eq!(only(None), std::slice::from_ref(&inside));
    assert_eq!(only(Some("launch")), std::slice::from_ref(&inside));
    assert_eq!(db.project(&project.id).unwrap().chat_count, 1);
    let copy = db.duplicate_conversation(&inside, None).unwrap();
    assert_eq!(
        db.conversation_project(&copy.id).unwrap().as_deref(),
        Some(project.id.as_str())
    );
    db.set_conversation_project(&outside, Some(&project.id))
        .unwrap();
    assert_eq!(only(None).len(), 3);
    db.set_project_archived(&project.id, true).unwrap();
    assert!(db
        .create_conversation(&NewConversation {
            project_id: Some(project.id.clone()),
            ..Default::default()
        })
        .is_err());
    assert!(db.projects(false).unwrap().is_empty());
    assert_eq!(db.projects(true).unwrap().len(), 1);
}

#[test]
fn tools_stay_inside_the_project_folders() {
    let (dir, db, folder, extra) = setup();
    let project = create(&db, &folder, &extra);
    assert_eq!(
        tool_workdir(&project, None).unwrap(),
        PathBuf::from(project.folder.clone().unwrap())
    );
    let sub = folder.join("src");
    std::fs::create_dir_all(&sub).unwrap();
    assert!(tool_workdir(&project, Some(&sub)).is_ok());
    assert!(tool_workdir(&project, Some(&extra)).is_ok());
    let elsewhere = dir.path().join("elsewhere");
    std::fs::create_dir_all(&elsewhere).unwrap();
    let refused = tool_workdir(&project, Some(&elsewhere)).unwrap_err();
    assert_eq!(
        crate::error_key::parse(&refused).unwrap().0,
        "projects.outside_folders"
    );
    // A link inside the project that points outside does not count as inside.
    #[cfg(unix)]
    {
        let link = folder.join("escape");
        std::os::unix::fs::symlink(&elsewhere, &link).unwrap();
        assert!(tool_workdir(&project, Some(&link)).is_err());
    }
    let grants = agent_grants(&project);
    assert_eq!(grants.read_folders.len(), 2);
    let bare = db
        .create_project(&ProjectInput {
            name: "No folder".into(),
            ..Default::default()
        })
        .unwrap();
    assert!(tool_workdir(&bare, None).is_err());
}

#[test]
fn outputs_are_saved_recorded_and_opened_inside_the_project() {
    let (dir, db, folder, extra) = setup();
    let project = create(&db, &folder, &extra);
    let conv = chat(&db, Some(&project.id), "draw");
    let from = OutputSource {
        source: "image".into(),
        model: Some("sd-model".into()),
        conversation_id: Some(conv.clone()),
        agent_task_id: None,
    };
    let first = save_output(&db, &project.id, "../cat.png", b"png", &from).unwrap();
    let second = save_output(&db, &project.id, "cat.png", b"png2", &from).unwrap();
    assert!(first.path.ends_with("cat.png"));
    assert!(second.path.ends_with("cat (2).png"));
    // The stored folder is the canonical one without the Windows `\?\` prefix.
    let canonical = crate::paths::display_path(&std::fs::canonicalize(&folder).unwrap());
    assert!(Path::new(&first.path).starts_with(Path::new(&canonical).join(OUTPUTS_DIR)));
    assert_eq!(first.kind, "image");
    assert_eq!(first.model.as_deref(), Some("sd-model"));
    assert_eq!(first.conversation_id.as_deref(), Some(conv.as_str()));
    assert_eq!(
        openable(&db, &first.id).unwrap(),
        PathBuf::from(&first.path)
    );

    std::fs::write(folder.join("notes.md"), "hello").unwrap();
    let tool = OutputSource {
        source: "tool".into(),
        model: Some("qwen".into()),
        conversation_id: Some(conv.clone()),
        agent_task_id: None,
    };
    let recorded = record_tool_output(
        &db,
        &project.id,
        "write_file",
        &serde_json::json!({"path": "notes.md"}),
        &folder,
        &tool,
    )
    .unwrap();
    assert_eq!(recorded.kind, "text");
    assert_eq!(recorded.tool.as_deref(), Some("write_file"));
    // Again: the same file is refreshed, not listed twice.
    record_tool_output(
        &db,
        &project.id,
        "edit_file",
        &serde_json::json!({"path": "notes.md"}),
        &folder,
        &tool,
    )
    .unwrap();
    assert_eq!(db.project_outputs(&project.id).unwrap().len(), 3);
    // Other tools and files outside the project are not outputs.
    let outside = dir.path().join("outside.txt");
    std::fs::write(&outside, "x").unwrap();
    assert!(record_tool_output(
        &db,
        &project.id,
        "read_file",
        &serde_json::json!({"path": "notes.md"}),
        &folder,
        &tool
    )
    .is_none());
    assert!(record_tool_output(
        &db,
        &project.id,
        "write_file",
        &serde_json::json!({"path": outside.to_string_lossy()}),
        &folder,
        &tool
    )
    .is_none());

    std::fs::remove_file(&second.path).unwrap();
    assert!(openable(&db, &second.id).is_err());

    // Archived projects take no new outputs.
    db.set_project_archived(&project.id, true).unwrap();
    assert!(save_output(&db, &project.id, "x.txt", b"x", &from).is_err());
}

#[test]
fn export_holds_manifest_chats_and_files() {
    let (dir, db, folder, extra) = setup();
    let project = create(&db, &folder, &extra);
    let conv = chat(&db, Some(&project.id), "hello");
    let from = OutputSource {
        source: "manual".into(),
        ..Default::default()
    };
    save_output(&db, &project.id, "a.txt", b"first", &from).unwrap();
    let gone = save_output(&db, &project.id, "b.txt", b"second", &from).unwrap();
    std::fs::remove_file(&gone.path).unwrap();
    let dest = dir.path().join("export.zip");
    assert_eq!(export_zip(&db, &project.id, &dest).unwrap(), 1);
    let mut archive = zip::ZipArchive::new(std::fs::File::open(&dest).unwrap()).unwrap();
    let mut manifest = String::new();
    archive
        .by_name("project.json")
        .unwrap()
        .read_to_string(&mut manifest)
        .unwrap();
    let manifest: serde_json::Value = serde_json::from_str(&manifest).unwrap();
    assert_eq!(manifest["schema"], EXPORT_SCHEMA);
    assert_eq!(manifest["project"]["name"], "Launch");
    assert_eq!(manifest["chats"][0], conv);
    let mut text = String::new();
    archive
        .by_name("outputs/a.txt")
        .unwrap()
        .read_to_string(&mut text)
        .unwrap();
    assert_eq!(text, "first");
    assert!(archive.by_name(&format!("chats/{conv}.json")).is_ok());
    // Entries carry real times, not the format's 1980 epoch.
    for i in 0..archive.len() {
        let entry = archive.by_index(i).unwrap();
        let year = entry.last_modified().map(|t| t.year()).unwrap_or(0);
        assert!(year >= 2024, "{:?} dated {year}", entry.name());
    }
    assert!(!dest.with_extension("zip.part").exists());
}

#[test]
fn delete_keeps_chats_and_optionally_the_files() {
    let (_dir, db, folder, extra) = setup();
    let from = OutputSource {
        source: "manual".into(),
        ..Default::default()
    };
    let keep = create(&db, &folder, &extra);
    let conv = chat(&db, Some(&keep.id), "kept");
    let kept_file = save_output(&db, &keep.id, "keep.txt", b"k", &from).unwrap();
    let report = delete(&db, &keep.id, false).unwrap();
    assert_eq!(report.unassigned_chats, 1);
    assert_eq!(report.kept_files, 1);
    assert!(Path::new(&kept_file.path).exists());
    assert_eq!(db.conversation_project(&conv).unwrap(), None);
    assert!(db.project(&keep.id).is_err());

    let remove = create(&db, &folder, &extra);
    let removed = save_output(&db, &remove.id, "remove.txt", b"r", &from).unwrap();
    let report = delete(&db, &remove.id, true).unwrap();
    assert_eq!(report.deleted_files, 1);
    assert!(!Path::new(&removed.path).exists());
    // The folders themselves stay.
    assert!(folder.join(OUTPUTS_DIR).is_dir());
    assert!(Path::new(&kept_file.path).exists());
}
