//! Library tests: real header metadata (extracted from local GGUF files into
//! `tests/data/gguf-library-hints.json` by `library_hints_dump`), and file operations on
//! temporary copies only.

use super::*;
use crate::domain::{ModelOrigin, ModelRef};
use crate::gguf::GgufMetadata;
use std::fs;
use std::io::Write;

fn data_file() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/data/gguf-library-hints.json")
}

/// Header facts of real files as `LocalModel`s (paths are fake; the files are not needed).
fn real_models() -> Vec<LocalModel> {
    let rows: Vec<serde_json::Value> =
        serde_json::from_str(&fs::read_to_string(data_file()).unwrap()).unwrap();
    rows.into_iter()
        .map(|row| {
            let meta: GgufMetadata = serde_json::from_value(row["meta"].clone()).unwrap();
            let file = row["file"].as_str().unwrap().to_string();
            let dir = row["dir"].as_str().unwrap().to_string();
            let path = std::path::PathBuf::from(format!("/lib/{}/{}", dir, file));
            LocalModel {
                id: path.to_string_lossy().to_string(),
                name: file.trim_end_matches(".gguf").to_string(),
                filename: file.clone(),
                path: path.clone(),
                size_bytes: row["size_bytes"].as_u64().unwrap(),
                architecture: meta.architecture.clone(),
                context_length: meta.context_length,
                quantization: meta.quantization.clone(),
                is_split: false,
                total_shards: None,
                present_shards: None,
                is_ready: true,
                shard_paths: vec![path],
                root_dir: "/lib".into(),
                kind: crate::domain::detect_kind(Some(&meta)),
                origin: ModelOrigin::Library,
                task: None,
                gguf: Some(meta),
                source: None,
                mmproj_path: None,
            }
        })
        .collect()
}

fn by_file<'a>(models: &'a [LocalModel], file: &str) -> &'a LocalModel {
    models.iter().find(|m| m.filename == file).unwrap()
}

#[test]
fn param_counts_on_real_headers() {
    let models = real_models();
    assert!(models.len() >= 10);
    for m in &models {
        let info = params::param_info(m);
        let exact = m.gguf.as_ref().unwrap().param_count.unwrap();
        // Complete single files: the tensor table gives the exact count.
        assert_eq!(info.count, Some(exact), "{}", m.filename);
        assert!(!info.estimated);
    }
    // Known sizes (general.size_label vs. tensor sum).
    let qwen4 = params::param_info(by_file(&models, "Qwen3-4B-Instruct-2507-Q4_K_M.gguf"));
    assert_eq!(qwen4.label.as_deref(), Some("4B"));
    assert!((4.0e9..4.1e9).contains(&(qwen4.count.unwrap() as f64)));
    let oss = params::param_info(by_file(&models, "gpt-oss-20b-MXFP4.gguf"));
    assert_eq!(oss.label.as_deref(), Some("32x2.4B"));
    assert!((20.8e9..21.0e9).contains(&(oss.count.unwrap() as f64)));
}

#[test]
fn param_estimates_without_tensor_table() {
    let models = real_models();
    // Estimates must land near the exact count for dense LLMs and embeddings.
    for m in models.iter().filter(|m| {
        matches!(m.kind, ModelKind::Llm | ModelKind::Embedding)
            && m.gguf.as_ref().unwrap().expert_count.is_none()
    }) {
        let mut meta = m.gguf.clone().unwrap();
        let exact = meta.param_count.take().unwrap() as f64;
        // From the label when it is a count ("4B", "567M", "0.6B").
        if let Some(n) = meta
            .size_label
            .as_deref()
            .and_then(params::parse_size_label)
        {
            let r = n as f64 / exact;
            assert!((0.8..1.25).contains(&r), "label {} {}", m.filename, r);
        }
        // From the shape (no label): within a factor of 1.6.
        let shape = params::estimate_from_shape(&meta).unwrap() as f64;
        let r = shape / exact;
        assert!((0.6..1.6).contains(&r), "shape {} {}", m.filename, r);
        // Through `param_info`: marked as an estimate.
        let mut copy = m.clone();
        copy.gguf = Some(meta);
        let info = params::param_info(&copy);
        assert!(info.estimated && info.count.is_some(), "{}", m.filename);
    }
    assert_eq!(params::parse_size_label("4B"), Some(4_000_000_000));
    assert_eq!(params::parse_size_label("0.6B"), Some(600_000_000));
    assert_eq!(params::parse_size_label("567M"), Some(567_000_000));
    assert_eq!(params::parse_size_label("32x2.4B"), None);
    assert_eq!(params::parse_size_label("large"), None);
    // Last resort: file size / bits per weight (Q4_K_M ~4.8 bpw).
    let n = params::estimate_from_size(2_497_280_256, "Q4_K_M").unwrap() as f64;
    assert!((3.5e9..4.6e9).contains(&n));
    assert_eq!(params::estimate_from_size(1, "Type_99"), None);
}

#[test]
fn draft_compatibility_on_real_tokenizers() {
    let models = real_models();
    let tok = |f: &str| {
        by_file(&models, f)
            .gguf
            .as_ref()
            .unwrap()
            .tokenizer
            .clone()
            .unwrap()
    };
    let qwen4 = tok("Qwen3-4B-Instruct-2507-Q4_K_M.gguf");
    let coder14 = tok("qwen2.5-coder-14b-instruct-q4_k_m.gguf");
    let oss = tok("gpt-oss-20b-MXFP4.gguf");
    let qwen36 = tok("Qwen3.6-27B-Q4_K_M.gguf");
    let gemma12 = tok("gemma-4-12B-it-Q4_K_M.gguf");
    let gemma26 = tok("gemma-4-26B-A4B-it-QAT-Q4_0.gguf");
    let qwen_emb = tok("Qwen3-Embedding-0.6B-Q8_0.gguf");

    // Qwen2.5 and Qwen3 share the BPE vocabulary (sizes 152064 vs 151936, within 128).
    assert_eq!(draft::tokenizers_compatible(&coder14, &qwen4), Ok(()));
    assert_eq!(coder14.head_hash, qwen4.head_hash);
    // Different pre-tokenizer / type / vocabulary.
    assert_eq!(
        draft::tokenizers_compatible(&coder14, &oss),
        Err(draft::Mismatch::PreTokenizer)
    );
    assert_eq!(
        draft::tokenizers_compatible(&qwen36, &qwen4),
        Err(draft::Mismatch::PreTokenizer)
    );
    assert_eq!(
        draft::tokenizers_compatible(&gemma26, &qwen4),
        Err(draft::Mismatch::TokenizerType)
    );
    // Same family and vocabulary.
    assert_eq!(draft::tokenizers_compatible(&gemma26, &gemma12), Ok(()));
    // The embedding model adds EOS: special-token handling differs from the chat model.
    assert_eq!(
        draft::tokenizers_compatible(&qwen4, &qwen_emb),
        Err(draft::Mismatch::SpecialTokens)
    );
    // Token texts are compared (a changed hash is a mismatch).
    let mut altered = qwen4.clone();
    altered.head_hash = Some("00".into());
    assert_eq!(
        draft::tokenizers_compatible(&coder14, &altered),
        Err(draft::Mismatch::TokenTexts)
    );

    // Suggestions: only LLMs, only much smaller ones, only matching tokenizers.
    let params: Vec<_> = models.iter().map(params::param_info).collect();
    let targets = draft::draft_targets(&models, &params);
    let idx = |f: &str| models.iter().position(|m| m.filename == f).unwrap();
    let q4 = &targets[idx("Qwen3-4B-Instruct-2507-Q4_K_M.gguf")];
    assert_eq!(
        q4,
        &vec![by_file(&models, "qwen2.5-coder-14b-instruct-q4_k_m.gguf")
            .id
            .clone()]
    );
    // Gemma 4 12B is ~46% of the 26B: too big to be a useful draft.
    assert!(targets[idx("gemma-4-12B-it-Q4_K_M.gguf")].is_empty());
    assert!(targets[idx("Qwen3-Embedding-0.6B-Q8_0.gguf")].is_empty());
    assert!(targets[idx("qwen2.5-coder-14b-instruct-q4_k_m.gguf")].is_empty());

    let suggested = draft::suggest_for(
        &by_file(&models, "qwen2.5-coder-14b-instruct-q4_k_m.gguf").path,
        &models,
    );
    assert_eq!(suggested.len(), 1);
    assert_eq!(suggested[0].name, "Qwen3-4B-Instruct-2507-Q4_K_M");
    assert!(suggested[0].ratio < 0.3);
}

#[test]
fn capabilities_and_projectors_on_real_headers() {
    let models = real_models();
    let entries = build_entries(models.clone(), &[], &[], &HashMap::new(), &["/lib".into()]);
    let e = |f: &str| entries.iter().find(|e| e.model.filename == f).unwrap();

    let gemma = e("gemma-4-12B-it-Q4_K_M.gguf");
    assert!(gemma.caps.tools && gemma.caps.reasoning && gemma.caps.vision && gemma.multimodal);
    assert!(
        gemma.caps.audio,
        "Gemma 4 12B projector has an audio encoder"
    );
    assert_eq!(gemma.pairs.len(), 1);
    let qwen36 = e("Qwen3.6-27B-Q4_K_M.gguf");
    assert!(qwen36.multimodal && qwen36.caps.reasoning && !qwen36.caps.audio);
    // Projector side lists its models.
    assert_eq!(
        e("mmproj-Qwen3.6-27B-BF16.gguf").pairs,
        vec![qwen36.model.id.clone()]
    );

    let qwen4 = e("Qwen3-4B-Instruct-2507-Q4_K_M.gguf");
    assert!(qwen4.caps.tools && !qwen4.caps.reasoning && !qwen4.multimodal);
    assert!(e("gpt-oss-20b-MXFP4.gguf").caps.reasoning);
    assert!(!e("qwen2.5-coder-14b-instruct-q4_k_m.gguf").caps.reasoning);
    let emb = e("bge-m3-q8_0.gguf");
    assert!(emb.caps.embedding && !emb.caps.chat && !emb.caps.tools);
    // A definition with a projector makes the base multimodal too.
    let mut def = ModelDefinition::new(
        "coder-vl",
        ModelRef::Local {
            path: e("qwen2.5-coder-14b-instruct-q4_k_m.gguf")
                .model
                .path
                .clone(),
        },
    );
    let dir = tempfile::tempdir().unwrap();
    let projector = dir.path().join("mmproj.gguf");
    write_metadata_gguf(&projector, "general.architecture", "clip");
    def.mmproj = Some(projector);
    let entries = build_entries(models, &[], &[def], &HashMap::new(), &[]);
    assert!(
        entries
            .iter()
            .find(|e| e.model.filename == "qwen2.5-coder-14b-instruct-q4_k_m.gguf")
            .unwrap()
            .multimodal
    );
}

#[test]
fn projector_suggestion_for_the_form() {
    let models = real_models();
    let llm = by_file(&models, "gemma-4-12B-it-Q4_K_M.gguf");
    let proj = by_file(&models, "mmproj-gemma-4-12B-it-BF16.gguf");
    let s = suggest_projector(&llm.path, &models).expect("projector next to the model");
    assert_eq!(s.path, proj.path);
    assert_eq!(s.name, proj.name);
    // A text-only model of another folder has none; so do projectors and unknown paths.
    assert_eq!(
        suggest_projector(
            &by_file(&models, "Qwen3-4B-Instruct-2507-Q4_K_M.gguf").path,
            &models
        ),
        None
    );
    assert_eq!(suggest_projector(&proj.path, &models), None);
    assert_eq!(
        suggest_projector(Path::new("/nowhere/x.gguf"), &models),
        None
    );

    // Two fitting projectors: the F16 one wins over BF16.
    let mut f16 = proj.clone();
    f16.filename = "mmproj-gemma-4-12B-it-F16.gguf".into();
    f16.name = "mmproj-gemma-4-12B-it-F16".into();
    f16.path = proj.path.with_file_name(&f16.filename);
    f16.id = f16.path.to_string_lossy().to_string();
    let mut both = models.clone();
    both.push(f16.clone());
    assert_eq!(suggest_projector(&llm.path, &both).unwrap().path, f16.path);

    // Linked from another app: its own projector wins, whatever sits next to it.
    let mut linked = models.clone();
    let own = std::path::PathBuf::from("/ollama/blobs/sha256-abc");
    linked
        .iter_mut()
        .find(|m| m.filename == llm.filename)
        .unwrap()
        .mmproj_path = Some(own.clone());
    assert_eq!(suggest_projector(&llm.path, &linked).unwrap().path, own);

    // Another model's projector in a flat library root is not offered.
    let mut flat = models.clone();
    let root = std::path::PathBuf::from("/flat");
    for m in flat
        .iter_mut()
        .filter(|m| m.filename == llm.filename || m.filename == "mmproj-Qwen3.6-27B-BF16.gguf")
    {
        m.path = root.join(&m.filename);
        m.root_dir = "/flat".into();
    }
    flat.iter_mut()
        .find(|m| m.filename == "mmproj-Qwen3.6-27B-BF16.gguf")
        .unwrap()
        .gguf
        .as_mut()
        .unwrap()
        .basename = Some("something-else".into());
    assert_eq!(suggest_projector(&root.join(&llm.filename), &flat), None);
}

#[test]
fn projector_matching_rules() {
    let models = real_models();
    let llm = by_file(&models, "gemma-4-12B-it-Q4_K_M.gguf");
    let proj = by_file(&models, "mmproj-gemma-4-12B-it-BF16.gguf");
    let other = by_file(&models, "mmproj-Qwen3.6-27B-BF16.gguf");
    assert!(projector_matches(llm, proj, 1, &[]));
    // Different folder: never.
    assert!(!projector_matches(llm, other, 1, &[]));
    // Same folder but a different model, in a flat library root: no.
    let mut moved = other.clone();
    moved.path = llm.path.with_file_name(&other.filename);
    moved.gguf.as_mut().unwrap().basename = Some("something-else".into());
    let root = llm.path.parent().unwrap().to_path_buf();
    assert!(!projector_matches(
        llm,
        &moved,
        2,
        std::slice::from_ref(&root)
    ));
    // Only projector in the model's own folder: yes.
    assert!(projector_matches(llm, &moved, 1, &[]));
}

// --- state, files ---

fn write_gguf(path: &std::path::Path, bytes: usize, fill: u8) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    let mut data = b"GGUF".to_vec();
    data.extend(std::iter::repeat_n(fill, bytes));
    fs::write(path, data).unwrap();
}

fn local(path: &std::path::Path, root: &std::path::Path) -> LocalModel {
    let models = crate::models::ModelManager.scan_directories(&[root.to_path_buf()]);
    models
        .into_iter()
        .find(|m| m.path == path)
        .unwrap_or_else(|| panic!("{} not scanned", path.display()))
}

#[test]
fn duplicates_are_grouped_by_size_and_edges() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let a = root.join("a/model-Q4_K_M.gguf");
    let b = root.join("b/model-copy-Q4_K_M.gguf");
    let c = root.join("c/other-Q4_K_M.gguf");
    let d = root.join("d/middle-differs-Q4_K_M.gguf");
    let size = 3 * dupes::EDGE_BYTES as usize;
    write_gguf(&a, size, 7);
    fs::create_dir_all(b.parent().unwrap()).unwrap();
    fs::copy(&a, &b).unwrap();
    write_gguf(&c, size, 9); // same size, different content
                             // Same size and edges, different middle: pre-hash matches, full hash does not.
    let mut mid = fs::read(&a).unwrap();
    mid[size / 2] ^= 0xff;
    fs::create_dir_all(d.parent().unwrap()).unwrap();
    fs::write(&d, mid).unwrap();

    let models = crate::models::ModelManager.scan_directories(&[root.to_path_buf()]);
    let groups = dupes::find_duplicates(&models);
    assert_eq!(groups.len(), 1);
    let names: Vec<&str> = groups[0].members.iter().map(|m| m.name.as_str()).collect();
    assert_eq!(names.len(), 3, "{names:?}");
    assert!(!names.contains(&"other-Q4_K_M"));

    // Full verification separates the file with a different middle.
    let members: Vec<LocalModel> = models
        .iter()
        .filter(|m| groups[0].members.iter().any(|x| x.model_id == m.id))
        .cloned()
        .collect();
    let verified = dupes::verify(&members, &HashMap::new()).unwrap();
    let id_of = |p: &std::path::Path| {
        members
            .iter()
            .find(|m| state_key(&m.path) == state_key(p))
            .unwrap()
            .id
            .clone()
    };
    let hash_of = |p: &std::path::Path| {
        verified
            .iter()
            .find(|v| v.model_id == id_of(p))
            .unwrap()
            .sha256
            .clone()
    };
    assert_eq!(hash_of(&a), hash_of(&b));
    assert_ne!(hash_of(&a), hash_of(&d));
    assert_eq!(hash_of(&a), crate::downloader::sha256_file(&a).unwrap());
    // A digest recorded at download time is used instead of reading the file.
    let mut recorded = HashMap::new();
    recorded.insert(id_of(&a), "ABCD".to_string());
    let v = dupes::verify(&members, &recorded).unwrap();
    let va = v.iter().find(|v| v.model_id == id_of(&a)).unwrap();
    assert!(va.recorded);
    assert_eq!(va.sha256, "abcd");
}

#[test]
fn state_db_v1_upgrades_to_v2_and_keeps_data() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("state.db");
    // A v1 database as the previous release wrote it, with a conversation in it.
    {
        let conn = rusqlite::Connection::open(&path).unwrap();
        conn.execute_batch(crate::state::schema_v1_sql()).unwrap();
        conn.pragma_update(None, "user_version", 1).unwrap();
        conn.execute(
            "INSERT INTO conversations(id, title, created_at, updated_at, model_alias, preset_id)
             VALUES ('c1', 'eski sohbet', 1, 5, 'Qwen-Coder', 'builtin:coding')",
            [],
        )
        .unwrap();
    }
    let db = StateDb::open(&path).unwrap();
    // v1 -> v2 (library tables) -> v3 (measured VRAM footprints) -> v4 (variant metadata)
    // in one open.
    assert_eq!(db.user_version().unwrap(), crate::state::SCHEMA_VERSION);
    const { assert!(crate::state::SCHEMA_VERSION >= 4) };
    assert!(db.vram_measurements().unwrap().is_empty());
    let list = db
        .list_conversations(&crate::state::ListQuery::default())
        .unwrap();
    assert_eq!(list.len(), 1);
    assert_eq!(list[0].title, "eski sohbet");

    // The new table works.
    let id = "D:\\Models\\x\\m.gguf";
    db.set_model_favorite(id, true).unwrap();
    db.record_model_use_at(id, Some("md-1"), Some("builtin:coding"), 100)
        .unwrap();
    db.note_models_added(&[(id.to_string(), 50), ("other".to_string(), 60)])
        .unwrap();
    db.note_models_added(&[(id.to_string(), 70)]).unwrap(); // first value wins
    let s = db.model_state(id).unwrap().unwrap();
    assert!(s.favorite);
    assert_eq!(s.last_used_at, Some(100));
    assert_eq!(s.last_definition_id.as_deref(), Some("md-1"));
    assert_eq!(s.added_at, Some(50));
    db.rename_model_state(id, "D:\\moved\\m.gguf").unwrap();
    assert!(db.model_state(id).unwrap().is_none());
    assert!(
        db.model_state("D:\\moved\\m.gguf")
            .unwrap()
            .unwrap()
            .favorite
    );
    db.forget_model_state("D:\\moved\\m.gguf").unwrap();
    assert_eq!(db.model_states().unwrap().len(), 1);
    // Latest conversation per alias (lower-cased).
    let uses = db.latest_alias_uses().unwrap();
    assert_eq!(
        uses["qwen-coder"].preset_id.as_deref(),
        Some("builtin:coding")
    );
    drop(db);
    // Re-opening is a no-op.
    assert_eq!(
        StateDb::open(&path).unwrap().user_version().unwrap(),
        crate::state::SCHEMA_VERSION
    );
}

#[test]
fn library_listing_joins_state_and_marks_missing_parts() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("lib");
    let single = root.join("owner/repo/single-Q4_K_M.gguf");
    write_gguf(&single, 64, 1);
    // Split set with part 2 of 3 missing, in the downloader's repo layout.
    for i in [1, 3] {
        write_gguf(
            &root.join(format!(
                "unsloth/Big-GGUF/Q4_K_M/Big-Q4_K_M-{:05}-of-00003.gguf",
                i
            )),
            64,
            2,
        );
    }
    // Same layout problem outside a repo folder.
    write_gguf(&root.join("Loose-Q8_0-00001-of-00002.gguf"), 64, 3);

    let db = StateDb::open(&dir.path().join("state.db")).unwrap();
    db.set_model_favorite(&state_key(&single), true).unwrap();
    let models = crate::models::ModelManager.scan_directories(std::slice::from_ref(&root));
    let view = list_from(models, &[], std::slice::from_ref(&root), Some(&db), None);
    assert!(view.state_error.is_none());
    let e = |name: &str| view.entries.iter().find(|e| e.model.name == name).unwrap();

    let s = e("single-Q4_K_M");
    assert!(s.favorite);
    assert!(s.added_at.is_some());
    assert!(s.shards.is_none() && s.repair.is_none());
    // Arrival time was stored for every file.
    assert_eq!(
        db.model_states()
            .unwrap()
            .iter()
            .filter(|s| s.added_at.is_some())
            .count(),
        3
    );

    let big = e("Big-Q4_K_M");
    let shards = big.shards.as_ref().unwrap();
    assert_eq!((shards.total, shards.present), (3, 2));
    assert_eq!(shards.missing, vec!["Big-Q4_K_M-00002-of-00003.gguf"]);
    let repair = big.repair.as_ref().unwrap();
    assert_eq!(repair.repo_id.as_deref(), Some("unsloth/Big-GGUF"));
    assert_eq!(
        repair.target_dir.as_deref(),
        Some(root.join("unsloth/Big-GGUF").as_path())
    );
    assert!(repair.reason.is_none());

    let loose = e("Loose-Q8_0");
    assert!(loose.repair.as_ref().unwrap().repo_id.is_none());
    assert!(loose.repair.as_ref().unwrap().reason.is_some());

    // The repo's variant reduced to the missing part (file list as the HF tree API gives it).
    let shard = |i: u32| crate::downloader::HfShardInfo {
        filename: format!("Q4_K_M/Big-Q4_K_M-{:05}-of-00003.gguf", i),
        download_url: String::new(),
        size_bytes: 10 * i as u64,
        shard_index: i,
        total_shards: 3,
        sha256: None,
    };
    let variants = vec![
        crate::downloader::HfModelVariant {
            not_runnable: None,
            name: "Big-Q8_0".into(),
            quantization: "Q8_0".into(),
            total_size_bytes: 0,
            is_split: true,
            shards: vec![crate::downloader::HfShardInfo {
                filename: "Q8_0/Big-Q8_0-00001-of-00002.gguf".into(),
                ..shard(1)
            }],
        },
        crate::downloader::HfModelVariant {
            not_runnable: None,
            name: "Q4_K_M/Big-Q4_K_M".into(),
            quantization: "Q4_K_M".into(),
            total_size_bytes: 60,
            is_split: true,
            shards: vec![shard(1), shard(2), shard(3)],
        },
    ];
    let subset = ops::missing_parts_variant(
        &big.model,
        repair.target_dir.as_ref().unwrap(),
        &variants,
        &shards.missing,
    )
    .unwrap();
    assert_eq!(subset.shards.len(), 1);
    assert_eq!(
        subset.shards[0].filename,
        "Q4_K_M/Big-Q4_K_M-00002-of-00003.gguf"
    );
    assert_eq!(subset.total_size_bytes, 20);
}

#[test]
fn delete_is_refused_outside_library_and_for_linked_models() {
    let dir = tempfile::tempdir().unwrap();
    let lib = dir.path().join("lib");
    let outside = dir.path().join("elsewhere");
    let inside = lib.join("a/in-Q4_K_M.gguf");
    let out = outside.join("out-Q4_K_M.gguf");
    write_gguf(&inside, 16, 1);
    write_gguf(&out, 16, 1);

    // A file outside the library directories is never deleted.
    let m_out = local(&out, &outside);
    assert!(ops::delete_files(&m_out, std::slice::from_ref(&lib)).is_err());
    assert!(out.exists());
    // Linked (Ollama / LM Studio) models: only the link can be removed.
    let mut linked = local(&inside, &lib);
    linked.origin = ModelOrigin::Linked;
    linked.source = Some("ollama".into());
    let err = ops::delete_files(&linked, std::slice::from_ref(&lib)).unwrap_err();
    assert_eq!(
        crate::error_key::parse(&err).unwrap().0,
        "library.linked_not_deletable"
    );
    assert!(inside.exists());
}

#[test]
fn delete_removes_every_part_definitions_on_request_and_state() {
    let dir = tempfile::tempdir().unwrap();
    let lib = dir.path().join("lib");
    for i in 1..=2 {
        write_gguf(
            &lib.join(format!("o/r/M-Q4_K_M-{:05}-of-00002.gguf", i)),
            16,
            4,
        );
    }
    let keep = lib.join("o/r/keep-Q4_K_M.gguf");
    write_gguf(&keep, 16, 5);
    let model = local(&lib.join("o/r/M-Q4_K_M-00001-of-00002.gguf"), &lib);
    assert_eq!(model.shard_paths.len(), 2);

    let domain = crate::domain::store::Domain::at(dir.path().join("config"));
    let base = domain
        .save_definition(ModelDefinition::new(
            "m-base",
            ModelRef::Local {
                path: model.path.clone(),
            },
        ))
        .unwrap();
    let mut other = ModelDefinition::new("keep-with-draft", ModelRef::Local { path: keep.clone() });
    other.draft = Some(crate::domain::DraftSpec {
        model: model.path.clone(),
        params: Default::default(),
    });
    let other = domain.save_definition(other).unwrap();
    let defs = domain.definitions().unwrap();

    let preview = ops::delete_preview(&model, &defs, &[], vec![]);
    assert!(!preview.linked);
    assert_eq!(preview.files.len(), 2);
    assert_eq!(preview.files.iter().map(|f| f.size_bytes).sum::<u64>(), 40);
    let roles: Vec<(String, ops::UseRole)> = preview
        .definitions
        .iter()
        .map(|d| (d.alias.clone(), d.role))
        .collect();
    assert!(roles.contains(&("m-base".into(), ops::UseRole::Base)));
    assert!(roles.contains(&("keep-with-draft".into(), ops::UseRole::Draft)));

    let db = StateDb::open(&dir.path().join("state.db")).unwrap();
    db.set_model_favorite(&state_key(&model.path), true)
        .unwrap();
    let report =
        ops::delete_model(&model, std::slice::from_ref(&lib), &domain, true, Some(&db)).unwrap();
    assert_eq!(report.files.len(), 2);
    assert!(model.shard_paths.iter().all(|p| !p.exists()));
    assert!(keep.exists());
    assert_eq!(report.definitions_deleted, vec!["m-base".to_string()]);
    assert!(domain.definition(&base.id).is_err());
    // A definition that only drafts with it stays (it will show the file as missing).
    assert!(domain.definition(&other.id).is_ok());
    assert!(db.model_state(&state_key(&model.path)).unwrap().is_none());
}

#[test]
fn move_keeps_state_and_rewrites_every_definition_reference() {
    let dir = tempfile::tempdir().unwrap();
    let lib = dir.path().join("lib");
    let target = dir.path().join("second");
    fs::create_dir_all(&target).unwrap();
    let file = lib.join("o/r/small-Q4_K_M.gguf");
    write_gguf(&file, 16, 6);
    let big = lib.join("o/r/big-Q4_K_M.gguf");
    write_gguf(&big, 16, 7);
    let model = local(&file, &lib);

    let domain = crate::domain::store::Domain::at(dir.path().join("config"));
    let mut d = ModelDefinition::new("big-with-draft", ModelRef::Local { path: big.clone() });
    d.draft = Some(crate::domain::DraftSpec {
        model: file.clone(),
        params: Default::default(),
    });
    d.adapters.push(crate::domain::AdapterRef {
        path: file.clone(),
        scale: None,
    });
    let d = domain.save_definition(d).unwrap();
    domain
        .save_definition(ModelDefinition::new(
            "small",
            ModelRef::Local { path: file.clone() },
        ))
        .unwrap();
    let db = StateDb::open(&dir.path().join("state.db")).unwrap();
    db.set_model_favorite(&model.id, true).unwrap();

    // Linked models are never moved.
    let mut linked = model.clone();
    linked.source = Some("lmstudio".into());
    assert!(
        crate::storage::StorageManager::migrate_model(&linked, &target.to_string_lossy()).is_err()
    );
    assert!(file.exists());

    let new_path =
        crate::storage::StorageManager::migrate_model(&model, &target.to_string_lossy()).unwrap();
    assert!(!file.exists() && new_path.exists());
    assert_eq!(new_path, target.join("o/r/small-Q4_K_M.gguf"));
    let new_id = new_path.to_string_lossy().to_string();
    domain
        .rename_model(&model.id, &new_id, |r| r.map(str::to_string))
        .unwrap();
    db.rename_model_state(&model.id, &new_id).unwrap();

    let moved = domain.definition(&d.id).unwrap();
    assert_eq!(moved.draft.unwrap().model, new_path);
    assert_eq!(moved.adapters[0].path, new_path);
    assert!(domain
        .definitions()
        .unwrap()
        .iter()
        .any(|x| x.alias == "small" && x.base.local_path() == Some(new_path.as_path())));
    assert!(db.model_state(&new_id).unwrap().unwrap().favorite);

    // Moving onto an existing file is refused and leaves both in place.
    write_gguf(&lib.join("o/r/small-Q4_K_M.gguf"), 16, 8);
    let again = local(&lib.join("o/r/small-Q4_K_M.gguf"), &lib);
    assert!(
        crate::storage::StorageManager::migrate_model(&again, &target.to_string_lossy()).is_err()
    );
    assert!(again.path.exists() && new_path.exists());
}

#[test]
fn gguf_parser_reads_tokenizer_and_tensor_table() {
    use std::io::Write;
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("t.gguf");
    let mut f = fs::File::create(&path).unwrap();
    let s = |f: &mut fs::File, v: &str| {
        f.write_all(&(v.len() as u64).to_le_bytes()).unwrap();
        f.write_all(v.as_bytes()).unwrap();
    };
    f.write_all(b"GGUF").unwrap();
    f.write_all(&3u32.to_le_bytes()).unwrap();
    f.write_all(&2u64.to_le_bytes()).unwrap(); // tensors
    f.write_all(&6u64.to_le_bytes()).unwrap(); // kv
    s(&mut f, "general.architecture");
    f.write_all(&8u32.to_le_bytes()).unwrap();
    s(&mut f, "llama");
    s(&mut f, "general.size_label");
    f.write_all(&8u32.to_le_bytes()).unwrap();
    s(&mut f, "1K");
    s(&mut f, "tokenizer.ggml.model");
    f.write_all(&8u32.to_le_bytes()).unwrap();
    s(&mut f, "gpt2");
    s(&mut f, "tokenizer.ggml.tokens");
    f.write_all(&9u32.to_le_bytes()).unwrap();
    f.write_all(&8u32.to_le_bytes()).unwrap();
    f.write_all(&7u64.to_le_bytes()).unwrap();
    for t in ["a", "b", "c", "d", "e", "f", "g"] {
        s(&mut f, t);
    }
    s(&mut f, "tokenizer.ggml.eos_token_id");
    f.write_all(&4u32.to_le_bytes()).unwrap();
    f.write_all(&2u32.to_le_bytes()).unwrap();
    s(&mut f, "tokenizer.ggml.add_bos_token");
    f.write_all(&7u32.to_le_bytes()).unwrap();
    f.write_all(&[1u8]).unwrap();
    // Tensor table: [4, 8] and [16].
    for (name, dims) in [("a.weight", vec![4u64, 8]), ("b.bias", vec![16])] {
        s(&mut f, name);
        f.write_all(&(dims.len() as u32).to_le_bytes()).unwrap();
        for d in dims {
            f.write_all(&d.to_le_bytes()).unwrap();
        }
        f.write_all(&0u32.to_le_bytes()).unwrap();
        f.write_all(&0u64.to_le_bytes()).unwrap();
    }
    drop(f);
    let meta = crate::gguf::read_gguf_metadata(&path).unwrap();
    assert_eq!(meta.param_count, Some(48));
    assert_eq!(meta.size_label.as_deref(), Some("1K"));
    let tok = meta.tokenizer.unwrap();
    assert_eq!(tok.model.as_deref(), Some("gpt2"));
    assert_eq!(tok.vocab_size, Some(7));
    assert_eq!(tok.eos_id, Some(2));
    assert_eq!(tok.add_bos, Some(true));
    assert_eq!(tok.head_hash, None, "vocabulary too small to fingerprint");
}

/// Writes `tests/data/gguf-library-hints.json` from real files:
/// HS_MODELS_DIR="D:\Models;C:\Users\me\.lmstudio\models" cargo test -p harness-core
///   library_hints_dump -- --ignored --nocapture
#[test]
#[ignore]
fn library_hints_dump() {
    let Ok(dirs) = std::env::var("HS_MODELS_DIR") else {
        eprintln!("HS_MODELS_DIR not set, skipping");
        return;
    };
    let mut files = Vec::new();
    for dir in dirs.split(';').filter(|d| !d.is_empty()) {
        crate::importer::collect_files(std::path::Path::new(dir), &mut files, 6);
    }
    files.sort();
    let mut rows = Vec::new();
    let mut dir_ids: Vec<String> = Vec::new();
    for f in files.iter().filter(|f| {
        f.extension()
            .is_some_and(|e| e.eq_ignore_ascii_case("gguf"))
    }) {
        let meta = crate::gguf::read_gguf_metadata(f).unwrap();
        // Folder identity only (no user paths in test data).
        let parent = f.parent().unwrap().to_string_lossy().to_string();
        let dir_no = match dir_ids.iter().position(|d| *d == parent) {
            Some(i) => i,
            None => {
                dir_ids.push(parent);
                dir_ids.len() - 1
            }
        };
        rows.push(serde_json::json!({
            "file": f.file_name().unwrap().to_string_lossy(),
            "dir": format!("d{}", dir_no),
            "size_bytes": fs::metadata(f).unwrap().len(),
            "meta": {
                "version": meta.version,
                "tensor_count": meta.tensor_count,
                "kv_count": meta.kv_count,
                "architecture": meta.architecture,
                "model_name": meta.model_name,
                "context_length": meta.context_length,
                "file_type": meta.file_type,
                "quantization": meta.quantization,
                "block_count": meta.block_count,
                "embedding_length": meta.embedding_length,
                "head_count": meta.head_count,
                "head_count_kv": meta.head_count_kv,
                "expert_count": meta.expert_count,
                "expert_used_count": meta.expert_used_count,
                "has_chat_template": meta.has_chat_template,
                "template_supports_tools": meta.template_supports_tools,
                "template_supports_reasoning": meta.template_supports_reasoning,
                "pooling_type": meta.pooling_type,
                "is_mmproj": meta.is_mmproj,
                "general_type": meta.general_type,
                "adapter_type": meta.adapter_type,
                "has_vision_encoder": meta.has_vision_encoder,
                "has_audio_encoder": meta.has_audio_encoder,
                "has_classifier_labels": meta.has_classifier_labels,
                "size_label": meta.size_label,
                "basename": meta.basename,
                "param_count": meta.param_count,
                "split_count": meta.split_count,
                "tokenizer": meta.tokenizer,
            },
        }));
    }
    println!("{}", serde_json::to_string_pretty(&rows).unwrap());
}

/// Library load path against a real llama-server: a definition made for a model file is
/// planned like the library's "Yükle" (new server), started, seen as using the file (delete
/// is refused), recorded as last used in `state.db`, then unloaded. Run with
/// HS_LLAMA_SERVER=<llama-server> HS_TEST_MODEL=<gguf> cargo test -p harness-core
///   e2e_library_load -- --ignored --nocapture
#[tokio::test]
#[ignore]
async fn e2e_library_load_unload_records_last_use() {
    let (Ok(bin), Ok(model)) = (
        std::env::var("HS_LLAMA_SERVER"),
        std::env::var("HS_TEST_MODEL"),
    ) else {
        eprintln!("HS_LLAMA_SERVER / HS_TEST_MODEL not set, skipping");
        return;
    };
    let model_path = std::path::PathBuf::from(&model);
    let domain = crate::domain::store::Domain::global();
    let mut def = ModelDefinition::new(
        format!("e2e-library-{}", std::process::id()),
        ModelRef::Local {
            path: model_path.clone(),
        },
    );
    def.load
        .insert("context_size".into(), crate::params::ParamValue::Int(1024));
    def.load
        .insert("gpu_layers".into(), crate::params::ParamValue::Int(99));
    def.default_preset_id = Some("builtin:coding".into());
    let def = domain.save_definition(def).unwrap();

    let registry = crate::servers::ServerRegistry::default();
    let plan = crate::orchestrator::plan_load_definition(
        &registry,
        &def.id,
        crate::orchestrator::LoadTarget::NewServer,
    )
    .await
    .unwrap();
    let crate::orchestrator::LoadPlan::Config {
        server_id,
        mut config,
    } = plan
    else {
        panic!("expected a new-server plan");
    };
    assert!(server_id.starts_with("m-e2e-library-"));
    config.host = "127.0.0.1".into();
    // A new server's plan leaves the port to the launch; this test pins its own.
    assert!(config.port_auto && config.port == 0);
    config.port = registry.suggest_port("127.0.0.1", 18291, &[]).unwrap();
    config.port_auto = false;
    let port = config.port;
    assert_eq!(config.definition_id.as_deref(), Some(def.id.as_str()));

    let before = crate::state::now_ms();
    let gate = crate::operation_gate::OperationGate::default();
    crate::orchestrator::launch_with_binary(
        &registry,
        &gate,
        server_id.clone(),
        *config,
        bin.into(),
        &crate::config::AppSettings::default(),
    )
    .await
    .unwrap();
    let pm = registry.get(&server_id).unwrap();
    let t = std::time::Instant::now();
    while pm.get_status() != crate::process::ServerStatus::Running {
        if t.elapsed() > std::time::Duration::from_secs(120) {
            let logs = pm.get_logs(Some(30)).join("\n");
            let _ = pm.stop().await;
            panic!("{:?}\n{}", pm.get_status(), logs);
        }
        tokio::time::sleep(std::time::Duration::from_millis(200)).await;
    }
    let using = ops::servers_using(&registry, std::slice::from_ref(&model_path));
    // Last use recorded by the launch hook.
    let db = crate::state::shared().unwrap();
    let state = db.model_state(&state_key(&model_path)).unwrap();

    pm.stop().await.unwrap();
    assert!(!pm.has_child());
    assert!(crate::servers::ensure_port_free("127.0.0.1", port).is_ok());
    let after_stop = ops::servers_using(&registry, std::slice::from_ref(&model_path));
    domain.delete_definition(&def.id).unwrap();

    assert_eq!(
        using,
        vec![server_id.clone()],
        "delete must be refused while running"
    );
    assert!(after_stop.is_empty());
    let state = state.expect("last use recorded");
    assert!(state.last_used_at.unwrap() >= before);
    assert_eq!(state.last_definition_id.as_deref(), Some(def.id.as_str()));
    assert_eq!(state.last_preset_id.as_deref(), Some("builtin:coding"));
    eprintln!("served {} on :{} and recorded {:?}", server_id, port, state);
}

/// The library shows the publisher of a downloaded file: `<library>/<owner>/<repo>/…` gives
/// `owner/repo`; files right in a folder of their own or outside the library give none.
#[test]
fn repo_is_read_from_the_download_folder() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let hf = root.join("unsloth/Qwen3-4B-GGUF/Qwen3-4B-Q4_K_M.gguf");
    let nested = root.join("bartowski/Llama-3.2-1B-GGUF/sub/Llama-3.2-1B-Q8_0.gguf");
    let loose = root.join("loose/model-Q4_K_M.gguf");
    for p in [&hf, &nested, &loose] {
        write_gguf(p, 64, 1);
    }
    let roots = [root.to_path_buf()];
    assert_eq!(
        ops::repo_of(&local(&hf, root), &roots).as_deref(),
        Some("unsloth/Qwen3-4B-GGUF")
    );
    assert_eq!(
        ops::repo_of(&local(&nested, root), &roots).as_deref(),
        Some("bartowski/Llama-3.2-1B-GGUF")
    );
    assert_eq!(ops::repo_of(&local(&loose, root), &roots), None);
    let elsewhere = tempfile::tempdir().unwrap();
    assert_eq!(
        ops::repo_of(&local(&hf, root), &[elsewhere.path().to_path_buf()]),
        None
    );
}

/// `/props` of a real Qwen3-4B-Instruct-2507 run (copied from `runtime_capabilities.json` of a
/// dev machine): tools yes, no reasoning level, reasoning of earlier turns not kept.
fn qwen_instruct_observation() -> crate::capabilities::ModelObservation {
    let caps = [
        ("supports_object_arguments", true),
        ("supports_parallel_tool_calls", true),
        ("supports_preserve_reasoning", false),
        ("supports_reasoning_effort", false),
        ("supports_string_content", true),
        ("supports_system_role", true),
        ("supports_tool_calls", true),
        ("supports_tools", true),
        ("supports_typed_content", false),
    ];
    crate::capabilities::ModelObservation {
        chat_template_caps: caps.iter().map(|(k, v)| (k.to_string(), *v)).collect(),
        observed_at: 1_790_852_515,
        ..Default::default()
    }
}

#[test]
fn props_observation_verifies_tools_and_raises_reasoning() {
    let models = real_models();
    let mut entries = build_entries(models, &[], &[], &HashMap::new(), &["/lib".into()]);
    let path_of = |entries: &[LibraryEntry], f: &str| {
        entries
            .iter()
            .find(|e| e.model.filename == f)
            .unwrap()
            .model
            .path
            .to_string_lossy()
            .to_string()
    };
    let qwen = path_of(&entries, "Qwen3-4B-Instruct-2507-Q4_K_M.gguf");
    let oss = path_of(&entries, "gpt-oss-20b-MXFP4.gguf");
    let emb = path_of(&entries, "bge-m3-q8_0.gguf");

    // gpt-oss reports a reasoning level; a template-only guess stays when /props is silent.
    let mut oss_obs = qwen_instruct_observation();
    oss_obs
        .chat_template_caps
        .insert("supports_reasoning_effort".into(), true);
    let embedding_obs = crate::capabilities::ModelObservation::default();
    let (qwen_k, oss_k, emb_k) = (
        crate::capabilities::model_key(&qwen),
        crate::capabilities::model_key(&oss),
        crate::capabilities::model_key(&emb),
    );
    apply_observations(&mut entries, &|p| {
        let k = crate::capabilities::model_key(p);
        if k == qwen_k {
            Some(qwen_instruct_observation())
        } else if k == oss_k {
            Some(oss_obs.clone())
        } else if k == emb_k {
            Some(embedding_obs.clone())
        } else {
            None
        }
    });
    let e = |f: &str| entries.iter().find(|e| e.model.filename == f).unwrap();

    let q = e("Qwen3-4B-Instruct-2507-Q4_K_M.gguf");
    assert_eq!(
        q.caps.verified,
        Some(VerifiedCaps {
            tools: true,
            reasoning: false,
            template_reasoning: None,
            observed_at: 1_790_852_515
        })
    );
    assert!(q.caps.tools && !q.caps.reasoning);
    let g = e("gpt-oss-20b-MXFP4.gguf");
    assert!(g.caps.reasoning && g.caps.verified.as_ref().unwrap().reasoning);
    // Never loaded: nothing verified, the template guess stays.
    let never = e("gemma-4-12B-it-Q4_K_M.gguf");
    assert!(never.caps.verified.is_none() && never.caps.reasoning);
    // An embedding run has no chat-template caps and the file is not an LLM anyway.
    assert!(e("bge-m3-q8_0.gguf").caps.verified.is_none());
    assert!(VerifiedCaps::from_observation(&embedding_obs).is_none());
}

/// A template that thinks but reports neither flag is not downgraded: `reasoning` stays true,
/// `verified.reasoning` says what the server reported.
#[test]
fn props_silence_does_not_take_back_template_reasoning() {
    let models = real_models();
    let mut entries = build_entries(models, &[], &[], &HashMap::new(), &["/lib".into()]);
    let gemma = entries
        .iter()
        .find(|e| e.model.filename == "gemma-4-12B-it-Q4_K_M.gguf")
        .unwrap()
        .model
        .path
        .to_string_lossy()
        .to_string();
    apply_observations(&mut entries, &|p| {
        (crate::capabilities::model_key(p) == crate::capabilities::model_key(&gemma))
            .then(qwen_instruct_observation)
    });
    let g = entries
        .iter()
        .find(|e| e.model.filename == "gemma-4-12B-it-Q4_K_M.gguf")
        .unwrap();
    assert!(g.caps.reasoning);
    assert!(!g.caps.verified.as_ref().unwrap().reasoning);
}

/// The server's own template is the fallback: a hybrid thinking model (real Qwen3-0.6B `/props`
/// reports neither flag) whose GGUF heuristic missed it still gets `reasoning`, but it is not
/// `verified.reasoning`; a template without the switches does not raise anything.
#[test]
fn props_template_inspection_raises_reasoning_unverified() {
    let models = real_models();
    let mut entries = build_entries(models, &[], &[], &HashMap::new(), &["/lib".into()]);
    let qwen = entries
        .iter()
        .find(|e| e.model.filename == "Qwen3-4B-Instruct-2507-Q4_K_M.gguf")
        .unwrap()
        .model
        .path
        .to_string_lossy()
        .to_string();
    let key = crate::capabilities::model_key(&qwen);
    let observe = |template_reasoning| {
        let key = key.clone();
        move |p: &str| {
            (crate::capabilities::model_key(p) == key).then(|| {
                crate::capabilities::ModelObservation {
                    template_reasoning,
                    ..qwen_instruct_observation()
                }
            })
        }
    };
    let find = |entries: &[LibraryEntry]| {
        entries
            .iter()
            .find(|e| e.model.filename == "Qwen3-4B-Instruct-2507-Q4_K_M.gguf")
            .unwrap()
            .caps
            .clone()
    };
    apply_observations(&mut entries, &observe(Some(false)));
    assert!(!find(&entries).reasoning);
    apply_observations(&mut entries, &observe(Some(true)));
    let caps = find(&entries);
    assert!(caps.reasoning, "the served template can think");
    let v = caps.verified.unwrap();
    assert!(!v.reasoning && v.template_reasoning == Some(true));
}

/// Real llama-server on the real gpt-oss-20b (CPU only, so it does not fight the dev app for
/// the GPU): the `/props` of the run is recorded in the capability registry and the library
/// shows reasoning as verified. Run with:
/// HS_LLAMA_SERVER=<llama-server> HS_TEST_REASONING_MODEL=<gpt-oss gguf>
/// cargo test -p harness-core library::tests::e2e_props -- --ignored --nocapture
#[tokio::test]
#[ignore]
async fn e2e_props_verifies_reasoning_of_a_real_model() {
    use crate::process::{ProcessManager, ServerStatus};
    let (Some(bin), Some(model)) = (
        std::env::var("HS_LLAMA_SERVER")
            .ok()
            .filter(|v| !v.is_empty()),
        std::env::var("HS_TEST_REASONING_MODEL")
            .ok()
            .filter(|v| !v.is_empty()),
    ) else {
        eprintln!("HS_LLAMA_SERVER / HS_TEST_REASONING_MODEL not set, skipping");
        return;
    };
    let port = crate::servers::ServerRegistry::default()
        .suggest_port("127.0.0.1", 18240, &[])
        .unwrap();
    let pm = ProcessManager::with_id("e2e-library-props");
    pm.start(crate::process::LaunchConfig {
        binary_path: bin.clone().into(),
        model_path: model.clone().into(),
        gpu_layers: 0,
        context_size: 2048,
        host: "127.0.0.1".into(),
        port,
        ..Default::default()
    })
    .await
    .unwrap();
    let t = std::time::Instant::now();
    while pm.effective_values().is_none() {
        if matches!(
            pm.get_status(),
            ServerStatus::Error(_) | ServerStatus::Stopped
        ) || t.elapsed() > std::time::Duration::from_secs(600)
        {
            let logs = pm.get_logs(Some(30)).join("\n");
            let _ = pm.stop().await;
            panic!("{:?}\n{logs}", pm.get_status());
        }
        tokio::time::sleep(std::time::Duration::from_millis(500)).await;
    }
    pm.stop().await.unwrap();

    let obs = crate::capabilities::registry()
        .observation_for(&model)
        .expect("the start recorded the /props observation");
    eprintln!("observed caps: {:?}", obs.chat_template_caps);
    let dir = std::path::Path::new(&model).parent().unwrap().to_path_buf();
    let models = crate::models::ModelManager.scan_directories(std::slice::from_ref(&dir));
    let view = list_from(models, &[], &[dir], None, None);
    let entry = view
        .entries
        .iter()
        .find(|e| {
            crate::capabilities::model_key(&e.model.path.to_string_lossy())
                == crate::capabilities::model_key(&model)
        })
        .expect("model listed");
    eprintln!("caps: {:?}", entry.caps);
    let v = entry.caps.verified.as_ref().expect("verified by /props");
    assert!(v.reasoning && entry.caps.reasoning && v.tools);
}

/// Repair sources: a recorded custom-URL download wins even in a repo-looking folder, a repo
/// folder uses Hugging Face, a moved Hugging Face file uses its record, an unknown file
/// explains itself and allows a user-given address, linked files are never touched.
#[test]
fn repair_info_picks_the_source() {
    use sources::ModelSource;
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().to_path_buf();
    let split = |sub: &str, stem: &str| {
        let d = root.join(sub);
        fs::create_dir_all(&d).unwrap();
        for i in [1, 3] {
            fs::write(d.join(format!("{stem}-{i:05}-of-00003.gguf")), b"GGUF").unwrap();
        }
        crate::models::ModelManager.scan_directories(std::slice::from_ref(&root))
    };
    let mut models = split("custom", "Cust-Q4");
    models.extend(split("o/r", "Repo-Q4"));
    models.extend(split("moved", "Moved-Q4"));
    models.extend(split("plain", "Plain-Q4"));
    let model = |name: &str| {
        models
            .iter()
            .find(|m| m.name.starts_with(name))
            .unwrap_or_else(|| {
                panic!(
                    "{name}: {:?}",
                    models.iter().map(|m| &m.name).collect::<Vec<_>>()
                )
            })
    };
    let record = |custom: bool, url: &str| ModelSource {
        url: url.into(),
        custom,
        at: 1,
    };
    let source_of = |p: &std::path::Path| {
        let name = p.file_name()?.to_string_lossy().to_string();
        if name.starts_with("Cust") || name.starts_with("Repo") {
            Some(record(true, &format!("https://example.com/{name}")))
        } else if name.starts_with("Moved") {
            Some(record(
                false,
                &format!(
                    "https://huggingface.co/o/r/resolve/refs%2Fpr%2F7/{name}?X-Amz-Signature=expired"
                ),
            ))
        } else {
            None
        }
    };
    let roots = std::slice::from_ref(&root);

    // Custom record: by address, even though `o/r` looks like a repo folder.
    let c = ops::repair_info_with(model("Cust"), roots, &source_of);
    assert_eq!(
        c.source_url.as_deref(),
        Some("https://example.com/Cust-Q4-00001-of-00003.gguf")
    );
    assert!(c.reason.is_none() && c.repo_id.is_none() && c.url_allowed);
    assert_eq!(c.target_dir.as_deref(), Some(root.join("custom").as_path()));
    let r = ops::repair_info_with(model("Repo"), roots, &source_of);
    assert!(r.repo_id.is_none() && r.source_url.is_some());
    // The same repo folder without a record: Hugging Face.
    let r = ops::repair_info_with(model("Repo"), roots, &|_| None);
    assert_eq!(r.repo_id.as_deref(), Some("o/r"));
    // A Hugging Face file moved out of its repo folder: its record.
    let m = ops::repair_info_with(model("Moved"), roots, &source_of);
    assert!(m.repo_id.is_none() && m.reason.is_none());
    assert_eq!(
        m.source_url.unwrap(),
        "https://huggingface.co/o/r/resolve/refs%2Fpr%2F7/Moved-Q4-00001-of-00003.gguf"
    );
    // Nothing known: explained, a user-given address allowed.
    let p = ops::repair_info_with(model("Plain"), roots, &source_of);
    assert!(p.url_allowed && p.source_url.is_none());
    assert!(p.reason.unwrap().starts_with("library.repair_no_source"));
    // Linked and outside-library files are refused for good.
    let mut linked = model("Plain").clone();
    linked.origin = ModelOrigin::Linked;
    let l = ops::repair_info_with(&linked, roots, &source_of);
    assert!(!l.url_allowed && l.reason.unwrap().starts_with("library.repair_linked"));
    let out = ops::repair_info_with(model("Plain"), &[], &source_of);
    assert!(
        !out.url_allowed
            && out
                .reason
                .unwrap()
                .starts_with("library.repair_outside_library")
    );
}

fn write_metadata_gguf(path: &std::path::Path, key: &str, value: &str) {
    let mut f = fs::File::create(path).unwrap();
    f.write_all(b"GGUF").unwrap();
    f.write_all(&3u32.to_le_bytes()).unwrap();
    f.write_all(&0u64.to_le_bytes()).unwrap();
    f.write_all(&2u64.to_le_bytes()).unwrap();
    for (k, v) in [("general.architecture", "llama"), (key, value)] {
        f.write_all(&(k.len() as u64).to_le_bytes()).unwrap();
        f.write_all(k.as_bytes()).unwrap();
        f.write_all(&8u32.to_le_bytes()).unwrap();
        f.write_all(&(v.len() as u64).to_le_bytes()).unwrap();
        f.write_all(v.as_bytes()).unwrap();
    }
}

#[test]
fn repair_info_infers_hf_repo_from_synthetic_gguf_metadata() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().to_path_buf();
    let folder = root.join("copied");
    fs::create_dir_all(&folder).unwrap();
    for i in [1, 3] {
        write_metadata_gguf(
            &folder.join(format!("Meta-Q4-0000{i}-of-00003.gguf")),
            "general.repo_url",
            "https://huggingface.co/acme/meta-gguf",
        );
    }
    let source_folder = root.join("copied-source");
    fs::create_dir_all(&source_folder).unwrap();
    for i in [1, 3] {
        write_metadata_gguf(
            &source_folder.join(format!("Source-Q4-0000{i}-of-00003.gguf")),
            "general.source.url",
            "https://huggingface.co/acme/source-gguf/resolve/dev/Source-Q4-00001-of-00003.gguf?sig=expired",
        );
    }
    let models = crate::models::ModelManager.scan_directories(std::slice::from_ref(&root));
    let model = models
        .iter()
        .find(|m| m.filename.starts_with("Meta-Q4"))
        .unwrap();
    let info = ops::repair_info_with(model, std::slice::from_ref(&root), &|_| None);
    assert_eq!(info.repo_id.as_deref(), Some("acme/meta-gguf"));
    assert_eq!(info.target_dir.as_deref(), Some(folder.as_path()));
    assert!(info.reason.is_none() && info.url_allowed);

    let source = models
        .iter()
        .find(|m| m.filename.starts_with("Source-Q4"))
        .unwrap();
    let info = ops::repair_info_with(source, std::slice::from_ref(&root), &|_| None);
    assert_eq!(
        info.source_url.as_deref(),
        Some("https://huggingface.co/acme/source-gguf/resolve/dev/Source-Q4-00001-of-00003.gguf")
    );
    assert_eq!(info.target_dir.as_deref(), Some(source_folder.as_path()));
}

/// Real Hugging Face split model (`ngxson/tinyllama_split_test`, 3 parts of stories15M, ~15 MB
/// in all). Parts 1 and 3 come in through custom-URL downloads, part 2 is missing: the repair
/// works from the recorded address, then (record removed) from a user-given address. Run with:
/// cargo test -p harness-core library::tests::e2e_repair -- --ignored --nocapture
#[tokio::test]
#[ignore] // network
async fn e2e_repair_split_model_from_url() {
    use crate::downloader::{DownloadManager, DownloadStatus};
    let base =
        "https://huggingface.co/ngxson/tinyllama_split_test/resolve/main/stories15M-q8_0-0000";
    let part = |i: u32| format!("{base}{i}-of-00003.gguf");
    let dm = DownloadManager::with_runtime(tokio::runtime::Handle::current());
    let wait = |id: u64| {
        let dm = dm.clone();
        async move {
            for _ in 0..240 {
                match dm.get(id).map(|p| p.status) {
                    Some(DownloadStatus::Completed) => return,
                    Some(DownloadStatus::Error(e)) => panic!("download {id}: {e}"),
                    _ => tokio::time::sleep(std::time::Duration::from_millis(500)).await,
                }
            }
            panic!("download {id} timed out");
        }
    };
    let root = DownloadManager::models_dir();
    let roots = vec![root.clone()];
    for i in [1, 3] {
        let id = dm
            .start_custom_url_download(part(i), None, None)
            .await
            .unwrap();
        wait(id).await;
    }
    let scan = || {
        crate::models::ModelManager
            .scan_directories(std::slice::from_ref(&root))
            .into_iter()
            .find(|m| m.filename.starts_with("stories15M-q8_0-00001"))
            .unwrap()
    };

    // 1) Recorded address.
    let model = scan();
    assert!(model.is_split && !model.is_ready);
    let info = ops::repair_info(&model, &roots);
    eprintln!("repair info: {info:?}");
    assert_eq!(info.source_url.as_deref(), Some(part(1).as_str()));
    let id = ops::repair(&dm, &model, &roots, None, None).await.unwrap();
    wait(id).await;
    let healed = scan();
    assert!(
        healed.is_ready && healed.present_shards == Some(3),
        "{healed:?}"
    );
    let size = std::fs::metadata(root.join("stories15M-q8_0-00002-of-00003.gguf"))
        .unwrap()
        .len();
    eprintln!("part 2 restored: {size} bytes");

    // 2) No record (as if copied by hand): explained, repaired with a given address.
    std::fs::remove_file(root.join("stories15M-q8_0-00002-of-00003.gguf")).unwrap();
    std::fs::remove_file(crate::paths::config_dir().join("model_sources.json")).unwrap();
    let model = scan();
    let info = ops::repair_info(&model, &roots);
    assert!(info.url_allowed && info.source_url.is_none());
    assert!(info
        .reason
        .as_deref()
        .unwrap()
        .starts_with("library.repair_no_source"));
    let no_url = ops::repair(&dm, &model, &roots, None, None).await;
    assert!(no_url.unwrap_err().starts_with("library.repair_no_source"));
    // An address of a different set is refused before anything is queued.
    let other = ops::repair(
        &dm,
        &model,
        &roots,
        None,
        Some("https://example.com/Other-00001-of-00003.gguf"),
    )
    .await;
    assert!(other
        .unwrap_err()
        .starts_with("library.repair_url_other_set"));
    let id = ops::repair(&dm, &model, &roots, None, Some(&part(1)))
        .await
        .unwrap();
    wait(id).await;
    assert!(scan().is_ready);
}

#[test]
fn registry_record_on_real_headers() {
    use crate::hub_compat::{Compatibility, ModelFormat};
    use crate::model_registry::Capability;

    let entries = build_entries(real_models(), &[], &[], &HashMap::new(), &["/lib".into()]);
    let e = |f: &str| entries.iter().find(|e| e.model.filename == f).unwrap();

    // A VLM: chat and vision, GGUF on llama.cpp (which is never reported missing).
    let gemma = &e("gemma-4-12B-it-Q4_K_M.gguf").record;
    assert!(gemma.capabilities.contains(&Capability::Chat));
    assert!(gemma.capabilities.contains(&Capability::Vision));
    assert!(gemma.capabilities.contains(&Capability::Audio));
    assert_eq!(gemma.format, ModelFormat::Gguf);
    assert_eq!(gemma.runtime.as_deref(), Some("llama.cpp"));
    let mut resolved = gemma.clone();
    resolved.resolve_installed(&[]);
    assert_eq!(
        resolved.compatibility,
        Some(Compatibility::Runnable {
            runtime: "llama.cpp".into()
        })
    );

    // Embedding and reranker files are not chat models.
    let emb = &e("bge-m3-q8_0.gguf").record;
    assert_eq!(emb.capabilities, vec![Capability::Embedding]);
    // The projector adds vision to its LLM but is not served on its own.
    let proj = &e("mmproj-Qwen3.6-27B-BF16.gguf").record;
    assert_eq!(proj.compatibility, None);
    assert!(proj.capabilities.contains(&Capability::Vision));
    assert!(!proj.capabilities.contains(&Capability::Chat));
}

#[test]
fn registry_record_of_a_whisper_file_needs_its_runtime() {
    use crate::hub_compat::{Compatibility, ModelFormat};
    use crate::model_registry::Capability;

    let mut m = real_models().remove(0);
    m.kind = ModelKind::Asr;
    m.gguf = None;
    m.filename = "ggml-base.bin".into();
    let entries = build_entries(vec![m], &[], &[], &HashMap::new(), &[]);
    let mut rec = entries[0].record.clone();
    assert!(entries[0].caps.asr && !entries[0].caps.chat);
    assert_eq!(rec.capabilities, vec![Capability::Asr]);
    assert_eq!(rec.format, ModelFormat::WhisperGgml);
    assert_eq!(rec.runtime.as_deref(), Some("whisper.cpp"));
    rec.resolve_installed(&[]);
    assert_eq!(
        rec.compatibility,
        Some(Compatibility::NeedsRuntime {
            runtime: "whisper.cpp".into()
        })
    );
    let mut installed = entries[0].record.clone();
    installed.resolve_installed(&["whisper.cpp".to_string()]);
    assert_eq!(
        installed.compatibility,
        Some(Compatibility::Runnable {
            runtime: "whisper.cpp".into()
        })
    );
}

#[test]
fn audio_mmproj_pairing_and_capability() {
    let mut llm = real_models()
        .into_iter()
        .find(|m| m.kind == ModelKind::Llm)
        .unwrap();
    llm.name = "Ultravox-7B-Q8_0".into();
    llm.path = PathBuf::from("/lib/org/ultravox/model/Ultravox-7B-Q8_0.gguf");
    llm.id = "audio-model".into();
    llm.quantization = "Q8_0".into();
    llm.gguf.as_mut().unwrap().basename = Some("Ultravox-7B".into());
    let mut proj = llm.clone();
    proj.kind = ModelKind::Mmproj;
    proj.id = "audio-f16".into();
    proj.name = "mmproj-Ultravox-7B-F16".into();
    proj.path = PathBuf::from("/lib/org/ultravox/mmproj-Ultravox-7B-F16.gguf");
    proj.quantization = "F16".into();
    proj.gguf.as_mut().unwrap().has_audio_encoder = true;
    proj.gguf.as_mut().unwrap().has_vision_encoder = false;
    let caps = caps_of(&proj, false, false);
    assert!(caps.audio_input);
    assert!(!caps.vision);
    let mut q8 = proj.clone();
    q8.id = "audio-q8".into();
    q8.name = "mmproj-Ultravox-7B-Q8_0".into();
    q8.path = PathBuf::from("/lib/org/ultravox/mmproj-Ultravox-7B-Q8_0.gguf");
    q8.quantization = "Q8_0".into();
    let models = vec![llm.clone(), proj.clone(), q8.clone()];
    assert_eq!(suggest_projector(&llm.path, &models).unwrap().path, q8.path);
    let entries = build_entries(models, &[], &[], &HashMap::new(), &[PathBuf::from("/lib")]);
    let entry = entries.iter().find(|e| e.model.id == llm.id).unwrap();
    assert!(entry.caps.audio_input && entry.multimodal);
    assert!(!entry.caps.vision);
    assert_eq!(entry.pairs.len(), 2);
    // With no matching Q8 projector the F16 companion is preferred.
    assert_eq!(
        suggest_projector(&llm.path, &[llm.clone(), proj.clone()])
            .unwrap()
            .path,
        proj.path
    );
    assert!(projector_matches(&llm, &proj, 2, &[PathBuf::from("/lib")]));
    proj.path = PathBuf::from("/lib/other/ultravox/mmproj-Ultravox-7B-F16.gguf");
    assert!(!projector_matches(&llm, &proj, 2, &[PathBuf::from("/lib")]));
}
