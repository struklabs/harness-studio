use super::*;

fn sample(id: u64, status: TrainingStatus) -> TrainingJob {
    let config = TrainingConfig {
        name: "t".into(),
        model: "/m".into(),
        dataset: "/d.jsonl".into(),
        method: Method::Lora,
        device: Device::Cpu,
        steps: 10,
        learning_rate: 2e-4,
        rank: 8,
        sequence_length: 128,
        validation_fraction: 0.1,
        checkpoint_steps: 5,
        eval_steps: 1,
        batch_size: 1,
        grad_accum: 1,
        lora_alpha: 16,
        lora_dropout: 0.05,
        warmup_ratio: 0.0,
        lr_scheduler: LrScheduler::Linear,
        weight_decay: 0.0,
        sample_prompts: vec![],
    };
    TrainingJob {
        id,
        total_steps: 10,
        config,
        status,
        step: 6,
        loss: None,
        eval_loss: None,
        checkpoint_step: None,
        training: true,
        error: None,
        dataset_report: dataset::inspect(Path::new("/none"), 0.1),
        estimate: estimate::estimate(
            &estimate::ModelShape {
                params: 1_000_000,
                hidden: 64,
                layers: 2,
                intermediate: 128,
                vocab: 100,
                quant_bits: None,
            },
            Method::Lora,
            Device::Cpu,
            8,
            128,
            10,
            50,
            1,
            1,
        ),
        export: None,
        resumed: false,
        created_at: 1,
        updated_at: 1,
        started_at: None,
        finished_at: None,
        paused: false,
    }
}

fn checkpoint(dir: &Path, step: u32, complete: bool) {
    let c = dir.join(format!("checkpoint-{step}"));
    std::fs::create_dir_all(&c).unwrap();
    std::fs::write(c.join("adapter_model.safetensors"), b"x").unwrap();
    if complete {
        std::fs::write(c.join("trainer_state.json"), b"{}").unwrap();
    }
}

#[test]
fn interrupted_jobs_load_as_failed_and_resumable() {
    let root = tempfile::tempdir().unwrap();
    for (id, status) in [
        (3, TrainingStatus::Running),
        (4, TrainingStatus::Done),
        (5, TrainingStatus::Queued),
    ] {
        let dir = root.path().join(id.to_string());
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("job.json"),
            serde_json::to_string(&sample(id, status)).unwrap(),
        )
        .unwrap();
    }
    checkpoint(&root.path().join("3"), 5, true);
    // A checkpoint cut off by a kill (no trainer state) is not a resume point.
    checkpoint(&root.path().join("3"), 8, false);
    let loaded = load_all(root.path());
    assert_eq!(loaded.len(), 3);
    let running = &loaded[0].job;
    assert_eq!(running.status, TrainingStatus::Failed);
    assert!(!running.training);
    assert_eq!(running.checkpoint_step, Some(5));
    assert_eq!(running.error.as_deref(), Some("training.interrupted"));
    assert_eq!(loaded[1].job.status, TrainingStatus::Done);
    assert_eq!(loaded[2].job.status, TrainingStatus::Failed);
    assert_eq!(loaded[2].job.checkpoint_step, None);
}

#[test]
fn metrics_merge_loss_and_eval_loss_per_step_and_later_runs_win() {
    let dir = tempfile::tempdir().unwrap();
    let p = dir.path().join("metrics.jsonl");
    std::fs::write(
        &p,
        "{\"step\":1,\"loss\":2.0}\n{\"step\":1,\"eval_loss\":2.5}\nnot json\n{\"step\":2,\"loss\":1.5}\n{\"step\":2,\"loss\":1.4}\n",
    )
    .unwrap();
    let m = run::read_metrics(&p);
    assert_eq!(
        m,
        vec![
            Metric {
                step: 1,
                loss: Some(2.0),
                eval_loss: Some(2.5),
                ..Default::default()
            },
            Metric {
                step: 2,
                loss: Some(1.4),
                eval_loss: None,
                ..Default::default()
            },
        ]
    );
    assert!(run::read_metrics(&dir.path().join("none")).is_empty());
}

#[test]
fn metrics_keep_checkpoint_samples_and_device_memory_at_the_same_step() {
    let dir = tempfile::tempdir().unwrap();
    let p = dir.path().join("metrics.jsonl");
    std::fs::write(
        &p,
        concat!(
            "{\"step\":5,\"loss\":1.2,\"vram_bytes\":100,\"vram_total_bytes\":200}\n",
            "{\"step\":5,\"samples\":[{\"prompt\":\"Hello\",\"text\":\"Hi\"}]}\n",
            "{\"step\":5,\"eval_loss\":1.3}\n",
            "{\"step\":5,\"samples\":[{\"prompt\":\"Hello\",\"text\":\"Resumed\"}]}\n"
        ),
    )
    .unwrap();
    let metrics = run::read_metrics(&p);
    assert_eq!(metrics.len(), 1);
    let m = &metrics[0];
    assert_eq!(m.loss, Some(1.2));
    assert_eq!(m.eval_loss, Some(1.3));
    assert_eq!(m.vram_bytes, Some(100));
    assert_eq!(m.vram_total_bytes, Some(200));
    assert_eq!(m.samples[0].text, "Resumed");
}

#[test]
fn prompts_are_bounded_and_old_requests_remain_readable() {
    let req: CreateRequest = serde_json::from_str(r#"{"model":"/m","dataset":"/d"}"#).unwrap();
    assert!(req.sample_prompts.is_none());
    assert!(validate_prompts(&[]).is_ok());
    assert!(validate_prompts(&["Hello".into()]).is_ok());
    assert!(validate_prompts(&[" ".into()]).is_err());
    assert!(validate_prompts(&vec!["Hello".into(); 9]).is_err());
    assert!(validate_prompts(&["a".repeat(2001)]).is_err());
    let mut old = serde_json::to_value(sample(100, TrainingStatus::Done)).unwrap();
    old.as_object_mut().unwrap().remove("paused");
    old["config"]
        .as_object_mut()
        .unwrap()
        .remove("sample_prompts");
    let restored: TrainingJob = serde_json::from_value(old).unwrap();
    assert!(!restored.paused);
    assert!(restored.config.sample_prompts.is_empty());
}

#[test]
fn slug_names_are_safe_folder_names() {
    assert_eq!(slug("SmolLM2 135M / LoRA!"), "smollm2-135m-lora");
    assert_eq!(slug("../../x"), "x");
    assert_eq!(slug("   "), "");
}

#[test]
fn interrupted_export_recovers_without_losing_completed_training() {
    let root = tempfile::tempdir().unwrap();
    let dir = root.path().join("8");
    std::fs::create_dir_all(&dir).unwrap();
    let mut job = sample(8, TrainingStatus::Done);
    job.export = Some(ExportInfo {
        status: ExportStatus::Running,
        dir: None,
        gguf: None,
        ggufs: vec![],
        warning: None,
        error: None,
        adapter_only: true,
    });
    std::fs::write(dir.join("job.json"), serde_json::to_vec(&job).unwrap()).unwrap();
    let loaded = load_all(root.path());
    assert_eq!(loaded[0].job.status, TrainingStatus::Done);
    let export = loaded[0].job.export.as_ref().unwrap();
    assert_eq!(export.status, ExportStatus::Failed);
    assert!(export.adapter_only);
    assert_eq!(export.error.as_deref(), Some("training.interrupted"));
}

#[test]
fn suggested_names_are_readable_and_unique() {
    let none = |_: &str| false;
    assert_eq!(
        suggest_with(
            Some("/models/qwen2.5-0.5b_instruct"),
            Some("/d/support-tone.jsonl"),
            none
        ),
        "Qwen2.5 0.5b Instruct Support Tone"
    );
    assert_eq!(suggest_with(Some("/m/smol"), None, none), "Smol LoRA");
    assert_eq!(suggest_with(None, Some("notes.csv"), none), "Notes");
    assert_eq!(suggest_with(None, None, none), "New Model");
    let taken = |n: &str| n == "Smol LoRA" || n == "Smol LoRA 2";
    assert_eq!(suggest_with(Some("/m/smol"), None, taken), "Smol LoRA 3");
    let long = "x".repeat(200);
    assert!(suggest_with(Some(&long), None, none).chars().count() <= 70);
}

/// An MLX checkpoint (`adapters.safetensors`) counts once its `trainer_state.json` is written.
#[test]
fn mlx_checkpoints_count_only_when_complete() {
    let dir = tempfile::tempdir().unwrap();
    for (step, complete) in [(4, true), (8, false)] {
        let c = dir.path().join(format!("checkpoint-{step}"));
        std::fs::create_dir_all(&c).unwrap();
        std::fs::write(c.join("adapters.safetensors"), b"x").unwrap();
        if complete {
            std::fs::write(c.join("trainer_state.json"), b"{}").unwrap();
        }
    }
    assert_eq!(run::latest_checkpoint(dir.path()), Some(4));
    assert!(run::has_adapter(&dir.path().join("checkpoint-8")));
}

/// Runs `code` with the system's Python next to the MLX worker (its data and checkpoint
/// helpers import no MLX); `None` without a Python 3.
fn mlx_worker_py(code: &str) -> Option<(bool, String)> {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("mlx_training_worker.py"),
        include_str!("../../python/mlx_training_worker.py"),
    )
    .unwrap();
    let python = if cfg!(windows) { "python" } else { "python3" };
    let code = format!(
        "import sys, json, pathlib; sys.path.insert(0, {:?}); import mlx_training_worker as w\n{code}",
        dir.path().to_string_lossy()
    );
    let out = std::process::Command::new(python)
        .args(["-I", "-c", &code])
        .output()
        .ok()?;
    let text =
        String::from_utf8_lossy(&out.stdout).to_string() + &String::from_utf8_lossy(&out.stderr);
    if !out.status.success() && text.contains("Python was not found") {
        return None;
    }
    Some((out.status.success(), text))
}

/// The MLX worker splits the snapshot like the job says (stable across runs), maps the
/// instruction format to chat, writes LoRA's scale as alpha / rank, and resumes only from
/// complete checkpoints.
#[test]
fn mlx_worker_splits_the_snapshot_and_configures_lora_like_the_job() {
    let Some((ok, text)) = mlx_worker_py(
        r#"
import tempfile
root = pathlib.Path(tempfile.mkdtemp())
rows = [{"messages": [{"role": "user", "content": str(i)}, {"role": "assistant", "content": "a"}]} for i in range(18)]
rows.append({"instruction": "hi", "input": "there", "output": "yo"})
rows.append({"instruction": "x", "output": "y"})
(root / "dataset.jsonl").write_text("\n".join(json.dumps(r) for r in rows) + "\n")
train, val = w.rows_to_chats(root, 0.1)
again, _ = w.rows_to_chats(root, 0.1)
assert (len(train), len(val)) == (18, 2) and train == again, (len(train), len(val))
assert all("messages" in r for r in train + val)
chats = [m for r in train + val for m in r["messages"]]
assert {"role": "user", "content": "hi\nthere"} in chats
cfg = {"rank": 8, "lora_alpha": 32, "lora_dropout": 0.1, "model": "/m", "method": "qlora"}
c = w.adapter_config(cfg, ["self_attn.q_proj"], True)
assert c["lora_parameters"] == {"rank": 8, "scale": 4.0, "dropout": 0.1, "keys": ["self_attn.q_proj"]}, c
assert c["num_layers"] == -1 and c["harness_quantized"] == {"group_size": 64, "bits": 4}
assert w.adapter_config({**cfg, "lora_alpha": 0}, [], False)["lora_parameters"]["scale"] == 2.0
for step, done in [(3, True), (6, False)]:
    d = root / f"checkpoint-{step}"
    d.mkdir()
    (d / "adapters.safetensors").write_bytes(b"x")
    if done:
        (d / "trainer_state.json").write_text("{}")
assert [s for s, _ in w.complete_checkpoints(root)] == [3]
base, out = root / "base", root / "out"
base.mkdir(); out.mkdir()
for name in ["tokenizer_config.json", "vocab.json", "merges.txt", "model.safetensors", "custom.py"]:
    (base / name).write_text("base " + name)
(out / "tokenizer_config.json").write_text("re-saved")
w.export_tokenizer(base, out)
assert (out / "tokenizer_config.json").read_text() == "base tokenizer_config.json"
assert (out / "merges.txt").is_file() and not (out / "model.safetensors").exists() and not (out / "custom.py").exists()
print("ok")
"#,
    ) else {
        return;
    };
    assert!(ok, "{text}");
}
