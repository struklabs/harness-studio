use super::scan_bench::tiny_gguf;
use super::*;

fn write(root: &Path, rel: &str, data: &[u8]) {
    let path = root.join(rel);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, data).unwrap();
}

// Original file-walk algorithm, retained only as a reference for discovery parity.
fn legacy_files(root: &Path) -> Vec<PathBuf> {
    let mut result = Vec::new();
    let mut visited = std::collections::HashSet::new();
    if let Ok(c) = fs::canonicalize(root) {
        visited.insert(c);
    }
    let mut stack = vec![(root.to_path_buf(), 0usize)];
    while let Some((dir, depth)) = stack.pop() {
        let Ok(entries) = fs::read_dir(dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            let Ok(md) = path.symlink_metadata() else {
                continue;
            };
            if md.file_type().is_symlink() {
                if path.is_file() && is_model_extension(&path) {
                    result.push(path);
                }
            } else if md.is_dir() {
                if path.file_name().and_then(|n| n.to_str()) == Some(".cache") {
                    continue;
                }
                let canon = fs::canonicalize(&path).unwrap_or_else(|_| path.clone());
                if visited.insert(canon) && depth < 8 {
                    stack.push((path, depth + 1));
                }
            } else if is_model_extension(&path) {
                result.push(path);
            }
        }
    }
    result
}

#[test]
fn single_walk_matches_legacy_mixed_library() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path();
    for name in [
        "owner/repo/single.gguf",
        "owner/repo/mmproj.gguf",
        "split/model-00001-of-00002.gguf",
        "split/model-00002-of-00002.gguf",
        ".hidden/visible-to-gguf.gguf",
        ".cache/ignored.gguf",
        "a/b/c/d/e/f/g/h/depth8.gguf",
        "a/b/c/d/e/f/g/h/i/too-deep.gguf",
    ] {
        tiny_gguf(&root.join(name));
    }
    super::scan_bench::tiny_gguf_arch(&root.join("owner/repo/mmproj.gguf"), "clip");
    // sherpa-onnx TTS and ASR fixtures (same files as the detector unit tests).
    for f in ["model.int8.onnx", "voices.bin", "tokens.txt"] {
        write(root, &format!("tts/kokoro-int8/{f}"), b"x");
    }
    for f in [
        "encoder.int8.onnx",
        "decoder.int8.onnx",
        "joiner.int8.onnx",
        "tokens.txt",
    ] {
        write(root, &format!("asr/parakeet/{f}"), b"x");
    }
    // Accepted models prune only their own detector's traversal, not other detectors.
    write(
        root,
        "tts/kokoro-int8/nested/config.json",
        br#"{"architectures":["BertForTokenClassification"]}"#,
    );
    write(root, "tts/kokoro-int8/nested/model.safetensors", b"x");
    for folder in ["checkpoints/chat", "checkpoints/mlx"] {
        write(root, &format!("{folder}/config.json"), br#"{"architectures":["LlamaForCausalLM"],"model_type":"llama","hidden_size":128,"num_hidden_layers":2}"#);
        write(root, &format!("{folder}/model.safetensors"), b"x");
    }
    write(
        root,
        "checkpoints/mlx/README.md",
        b"---\nlibrary_name: mlx\n---\n",
    );
    write(
        root,
        "tasks/classifier/config.json",
        br#"{"architectures":["BertForSequenceClassification"],"id2label":{"0":"NEG","1":"POS"}}"#,
    );
    write(root, "tasks/classifier/model.safetensors", b"x");
    write(
        root,
        "tasks/image/model_index.json",
        br#"{"_class_name":"StableDiffusionPipeline"}"#,
    );
    write(
        root,
        "tasks/image/unet/diffusion_pytorch_model.safetensors",
        b"x",
    );
    // Hidden folder detectors must stay hidden even though the GGUF walk enters them.
    write(
        root,
        ".hidden/task/config.json",
        br#"{"architectures":["BertForTokenClassification"]}"#,
    );
    write(root, ".hidden/task/model.safetensors", b"x");
    let roots = [root.to_path_buf()];
    let mut legacy = ModelManager.scan_files(root, legacy_files(root));
    legacy.extend(crate::tts::library_models(&roots));
    legacy.extend(crate::asr::library_models(&roots));
    legacy.extend(crate::engine::mistralrs::library_models(&roots));
    legacy.extend(crate::engine::python_hf::library_models(&roots));
    legacy.extend(crate::engine::mlx::library_models(&roots));
    legacy.sort_by(|a, b| a.name.cmp(&b.name));
    let current = ModelManager.scan_directories(&roots);
    assert!(current
        .iter()
        .any(|m| m.kind == crate::domain::ModelKind::Tts));
    assert!(current
        .iter()
        .any(|m| m.kind == crate::domain::ModelKind::Asr));
    assert!(current.iter().any(|m| m.path.ends_with("chat")));
    assert!(current.iter().any(|m| m.path.ends_with("classifier")));
    // Compare every field, plus order; no filesystem timestamps are part of LocalModel.
    assert_eq!(
        serde_json::to_value(current).unwrap(),
        serde_json::to_value(legacy).unwrap()
    );
}

#[test]
fn shared_scan_is_fresh_after_delete_move_and_add_directory() {
    let temp = tempfile::tempdir().unwrap();
    let primary = temp.path().join("primary");
    let extra = temp.path().join("extra");
    tiny_gguf(&primary.join("owner/repo/a.gguf"));
    tiny_gguf(&extra.join("owner/repo/b.gguf"));
    let mut roots = vec![primary.clone()];
    let first = scan_cache::scan(&roots);
    let a = first.iter().find(|m| m.path.starts_with(&primary)).unwrap();
    crate::library::ops::delete_files(a, &roots).unwrap();
    assert!(!scan_cache::scan(&roots).iter().any(|m| m.id == a.id));
    // Adding/removing directories changes the cache key immediately, including without save.
    roots.push(extra.clone());
    let added = scan_cache::scan(&roots);
    let b = added.iter().find(|m| m.path.starts_with(&extra)).unwrap();
    assert_eq!(b.name, "b");
    crate::storage::StorageManager::migrate_model(b, primary.to_str().unwrap()).unwrap();
    let moved = scan_cache::scan(&roots);
    assert!(!moved.iter().any(|m| m.path == b.path));
    assert!(moved
        .iter()
        .any(|m| m.path == primary.join("owner/repo/b.gguf")));
    roots.pop();
    assert!(scan_cache::scan(&roots)
        .iter()
        .all(|m| !m.path.starts_with(&extra)));
}

#[test]
fn metadata_cache_hits_and_checks_size_and_mtime() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("tiny.gguf");
    tiny_gguf(&path);
    METADATA_STATS.with(|s| s.set((0, 0)));
    assert!(cached_metadata(&path).is_some());
    assert!(cached_metadata(&path).is_some());
    METADATA_STATS.with(|s| assert_eq!(s.get(), (1, 1)));
    let file = fs::OpenOptions::new().write(true).open(&path).unwrap();
    let changed = std::time::SystemTime::now() + std::time::Duration::from_secs(10);
    file.set_modified(changed).unwrap();
    assert!(cached_metadata(&path).is_some());
    file.set_len(file.metadata().unwrap().len() + 1).unwrap();
    assert!(cached_metadata(&path).is_some());
    METADATA_STATS.with(|s| assert_eq!(s.get(), (1, 3)));
}
