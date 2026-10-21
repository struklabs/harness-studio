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
            },
            Method::Lora,
            Device::Cpu,
            8,
            128,
            10,
            50,
        ),
        export: None,
        resumed: false,
        created_at: 1,
        updated_at: 1,
        started_at: None,
        finished_at: None,
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
                eval_loss: Some(2.5)
            },
            Metric {
                step: 2,
                loss: Some(1.4),
                eval_loss: None
            },
        ]
    );
    assert!(run::read_metrics(&dir.path().join("none")).is_empty());
}

#[test]
fn slug_names_are_safe_folder_names() {
    assert_eq!(slug("SmolLM2 135M / LoRA!"), "smollm2-135m-lora");
    assert_eq!(slug("../../x"), "x");
    assert_eq!(slug("   "), "");
}
