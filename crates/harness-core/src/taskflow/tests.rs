use super::*;
use crate::state::{NewTaskRun, Price, ProjectInput, RunPolicy, StepInput};
use axum::{extract::State, http::StatusCode, routing::get, routing::post, Json, Router};
use serde_json::{json, Value};
use std::sync::{
    atomic::{AtomicBool, AtomicUsize, Ordering},
    Arc,
};
use std::time::Duration;

/// A fake OpenAI-compatible endpoint: `bad` always answers 500, `flaky` fails its first two
/// requests with 503, `slow` sleeps, everything else answers `ok:<model>`.
#[derive(Default)]
struct Fake {
    calls: AtomicUsize,
    flaky_calls: AtomicUsize,
    slow_ms: AtomicUsize,
    seen_models: std::sync::Mutex<Vec<String>>,
}

async fn models() -> Json<Value> {
    Json(json!({"object": "list", "data": [
        {"id": "good", "harness": {"state": "ready", "context_size": 8192}},
        {"id": "bad", "harness": {"state": "ready", "context_size": 8192}},
        {"id": "flaky", "harness": {"state": "ready", "context_size": 8192}},
        {"id": "tiny", "harness": {"state": "ready", "context_size": 2048}},
        {"id": "slow", "harness": {"state": "ready"}},
        {"id": "cloud:x/m", "harness": {"state": "ready", "cloud": {
            "context_length": 128000, "tools": true, "input_price": 2.0, "output_price": 10.0}}},
        {"id": "cloud:x/notools", "harness": {"state": "ready", "cloud": {
            "context_length": 128000, "tools": false}}},
    ]}))
}

async fn chat(State(f): State<Arc<Fake>>, Json(body): Json<Value>) -> (StatusCode, Json<Value>) {
    f.calls.fetch_add(1, Ordering::SeqCst);
    let model = body["model"].as_str().unwrap_or_default().to_string();
    f.seen_models.lock().unwrap().push(model.clone());
    let ms = f.slow_ms.load(Ordering::SeqCst);
    if model == "slow" && ms > 0 {
        tokio::time::sleep(Duration::from_millis(ms as u64)).await;
    }
    if model == "bad" {
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"error": {"message": "boom"}})),
        );
    }
    if model == "flaky" && f.flaky_calls.fetch_add(1, Ordering::SeqCst) < 2 {
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(json!({"error": {"message": "loading"}})),
        );
    }
    let last = body["messages"].as_array().and_then(|m| m.last()).cloned();
    let prompt = last
        .as_ref()
        .and_then(|m| m["content"].as_str())
        .unwrap_or("");
    (
        StatusCode::OK,
        Json(json!({
            "choices": [{"message": {"role": "assistant", "content": format!("ok:{model}:{prompt}")}}],
            "usage": {"prompt_tokens": 1000, "completion_tokens": 500},
        })),
    )
}

async fn serve() -> (Endpoint, Arc<Fake>) {
    let fake = Arc::new(Fake::default());
    let app = Router::new()
        .route("/v1/models", get(models))
        .route("/v1/chat/completions", post(chat))
        .with_state(fake.clone());
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    (
        Endpoint {
            base_url: format!("http://127.0.0.1:{port}/v1"),
            ..Default::default()
        },
        fake,
    )
}

fn db() -> (tempfile::TempDir, Arc<StateDb>) {
    let dir = tempfile::tempdir().unwrap();
    let db = Arc::new(StateDb::open(&dir.path().join("state.db")).unwrap());
    (dir, db)
}

fn steps(n: usize) -> Vec<StepInput> {
    (1..=n)
        .map(|i| StepInput {
            title: format!("S{i}"),
            prompt: format!("do {i}"),
            gate: false,
        })
        .collect()
}

fn new_run(db: &StateDb, models: &[&str], n: usize) -> TaskRun {
    db.create_task_run(&NewTaskRun {
        title: "Run".into(),
        goal: "Test goal".into(),
        policy: RunPolicy {
            models: models.iter().map(|m| m.to_string()).collect(),
            backoff_ms: 10,
            ..Default::default()
        },
        steps: steps(n),
        ..Default::default()
    })
    .unwrap()
}

fn opts(ep: &Endpoint) -> RunOptions {
    RunOptions::new(ep.clone())
}

fn flag() -> Arc<AtomicBool> {
    Arc::new(AtomicBool::new(false))
}

#[tokio::test]
async fn drafts_are_editable_and_do_not_run() {
    let (_d, db) = db();
    let run = new_run(&db, &["good"], 2);
    assert_eq!(run.status, "draft");
    let mut input = NewTaskRun {
        title: "Renamed".into(),
        policy: run.policy.clone(),
        steps: steps(3),
        ..Default::default()
    };
    let edited = db.update_task_run(&run.id, &input).unwrap();
    assert_eq!(edited.title, "Renamed");
    assert_eq!(db.task_steps(&run.id).unwrap().len(), 3);
    input.steps.clear();
    assert!(db.update_task_run(&run.id, &input).is_err());
}

#[tokio::test]
async fn runs_all_steps_with_checkpoints_and_tally() {
    let (_d, db) = db();
    let (ep, _fake) = serve().await;
    let mut run = new_run(&db, &["good"], 3);
    run.policy.price = Some(Price {
        input_per_m: 1.0,
        output_per_m: 2.0,
    });
    db.update_task_run(
        &run.id,
        &NewTaskRun {
            title: "Run".into(),
            policy: run.policy.clone(),
            steps: steps(3),
            ..Default::default()
        },
    )
    .unwrap();
    let done = run_task(db.clone(), &run.id, opts(&ep), flag())
        .await
        .unwrap();
    assert_eq!(done.status, "completed");
    let s = db.task_steps(&run.id).unwrap();
    assert!(s
        .iter()
        .all(|s| s.status == "done" && s.output.starts_with("ok:good")));
    assert_eq!(db.task_checkpoints(&run.id).unwrap().len(), 3);
    // 3 steps x (1000 in, 500 out) at $1/$2 per million = 3 x 2000 micro-USD.
    assert_eq!(
        (done.tokens_in, done.tokens_out, done.cost_micro),
        (3000, 1500, 6000)
    );
}

#[tokio::test]
async fn falls_back_with_a_reason_and_retries_with_backoff() {
    let (_d, db) = db();
    let (ep, fake) = serve().await;
    // `ghost` is not served, `tiny` is below min_context, `bad` fails for good, `flaky`
    // succeeds on its third attempt.
    let mut run = new_run(&db, &["ghost", "tiny", "bad", "flaky"], 1);
    run.policy.min_context = Some(4096);
    db.update_task_run(
        &run.id,
        &NewTaskRun {
            title: "Run".into(),
            policy: run.policy.clone(),
            steps: steps(1),
            ..Default::default()
        },
    )
    .unwrap();
    let done = run_task(db.clone(), &run.id, opts(&ep), flag())
        .await
        .unwrap();
    assert_eq!(done.status, "completed");
    let step = &db.task_steps(&run.id).unwrap()[0];
    assert_eq!(step.model.as_deref(), Some("flaky"));
    let reason = step.fallback_reason.clone().unwrap();
    assert!(reason.contains("taskrun.reason.unavailable"), "{reason}");
    assert!(reason.contains("taskrun.reason.context_small"), "{reason}");
    assert!(reason.contains("taskrun.reason.request_failed"), "{reason}");
    // bad: 3 attempts (500 is retryable); flaky: 3 attempts.
    assert_eq!(step.attempts, 6);
    assert_eq!(fake.calls.load(Ordering::SeqCst), 6);
    let events = db.task_events(&run.id).unwrap();
    assert!(events.iter().filter(|e| e.kind == "retry").count() >= 4);
}

#[tokio::test]
async fn fails_when_no_model_meets_the_requirements() {
    let (_d, db) = db();
    let (ep, _fake) = serve().await;
    let mut run = new_run(&db, &["tiny", "ghost"], 1);
    run.policy.min_context = Some(4096);
    db.update_task_run(
        &run.id,
        &NewTaskRun {
            title: "Run".into(),
            policy: run.policy.clone(),
            steps: steps(1),
            ..Default::default()
        },
    )
    .unwrap();
    let checks = preflight(&db, &run.id, &ep).await.unwrap();
    assert!(checks.iter().all(|c| !c.ok));
    let done = run_task(db.clone(), &run.id, opts(&ep), flag())
        .await
        .unwrap();
    assert_eq!(done.status, "failed");
    assert_eq!(done.error.as_deref(), Some("taskrun.no_model_qualifies"));
    // A failed run resumes once the policy is fixed.
    db.update_task_run(
        &run.id,
        &NewTaskRun {
            title: "Run".into(),
            policy: RunPolicy {
                models: vec!["good".into()],
                ..Default::default()
            },
            steps: steps(1),
            ..Default::default()
        },
    )
    .unwrap();
    let done = run_task(db.clone(), &run.id, opts(&ep), flag())
        .await
        .unwrap();
    assert_eq!(done.status, "completed");
}

#[tokio::test]
async fn tool_requirement_uses_declared_support_and_cloud_prices() {
    let (_d, db) = db();
    let (ep, _fake) = serve().await;
    let mut run = new_run(&db, &["cloud:x/notools", "cloud:x/m"], 1);
    run.policy.require_tools = true;
    db.update_task_run(
        &run.id,
        &NewTaskRun {
            title: "Run".into(),
            policy: run.policy.clone(),
            steps: steps(1),
            ..Default::default()
        },
    )
    .unwrap();
    let checks = preflight(&db, &run.id, &ep).await.unwrap();
    assert!(!checks[0].ok);
    assert!(checks[0].reason.as_deref().unwrap().contains("no_tools"));
    assert!(checks[1].ok);
    let done = run_task(db.clone(), &run.id, opts(&ep), flag())
        .await
        .unwrap();
    // 1000 in x $2 + 500 out x $10 per million, from the provider's price list.
    assert_eq!(done.cost_micro, 7000);
    assert_eq!(done.last_model.as_deref(), Some("cloud:x/m"));
}

#[tokio::test]
async fn step_mode_pauses_and_rewind_runs_steps_again() {
    let (_d, db) = db();
    let (ep, _fake) = serve().await;
    let run = new_run(&db, &["good"], 3);
    let mut o = opts(&ep);
    o.action = RunAction::Step;
    let r = run_task(db.clone(), &run.id, o.clone(), flag())
        .await
        .unwrap();
    assert_eq!(r.status, "paused");
    assert_eq!(
        db.task_steps(&run.id)
            .unwrap()
            .iter()
            .filter(|s| s.status == "done")
            .count(),
        1
    );
    run_task(db.clone(), &run.id, o, flag()).await.unwrap();
    let r = run_task(db.clone(), &run.id, opts(&ep), flag())
        .await
        .unwrap();
    assert_eq!(r.status, "completed");
    let spent = r.tokens_in;
    // Back to before step 2: steps 2 and 3 are pending, step 1 keeps its output.
    let r = db.rewind_task_run(&run.id, 1).unwrap();
    assert_eq!(r.status, "paused");
    let s = db.task_steps(&run.id).unwrap();
    assert_eq!(s[0].status, "done");
    assert!(s[1].output.is_empty() && s[1].status == "pending");
    assert_eq!(db.task_checkpoints(&run.id).unwrap().len(), 1);
    let r = run_task(db.clone(), &run.id, opts(&ep), flag())
        .await
        .unwrap();
    assert_eq!(r.status, "completed");
    // The tally keeps what the discarded steps cost.
    assert_eq!(r.tokens_in, spent + 2000);
}

#[tokio::test]
async fn approval_gate_blocks_until_granted() {
    let (_d, db) = db();
    let (ep, _fake) = serve().await;
    let mut input = NewTaskRun {
        title: "Gated".into(),
        policy: RunPolicy {
            models: vec!["good".into()],
            ..Default::default()
        },
        steps: steps(2),
        ..Default::default()
    };
    input.steps[1].gate = true;
    let run = db.create_task_run(&input).unwrap();
    let r = run_task(db.clone(), &run.id, opts(&ep), flag())
        .await
        .unwrap();
    assert_eq!(r.status, "awaiting_approval");
    let s = db.task_steps(&run.id).unwrap();
    assert_eq!(
        (s[0].status.as_str(), s[1].approval.as_str()),
        ("done", "pending")
    );
    db.set_task_step_approval(&run.id, 1, "granted").unwrap();
    let r = run_task(db.clone(), &run.id, opts(&ep), flag())
        .await
        .unwrap();
    assert_eq!(r.status, "completed");
}

#[tokio::test]
async fn a_dropped_runner_is_recovered_and_resumed() {
    let (_d, db) = db();
    let (ep, fake) = serve().await;
    fake.slow_ms.store(60_000, Ordering::SeqCst);
    let run = new_run(&db, &["good", "slow"], 3);
    // Step 1 runs on `good`; make step 2 hang by putting `slow` first from then on.
    let mut o = opts(&ep);
    o.action = RunAction::Step;
    run_task(db.clone(), &run.id, o, flag()).await.unwrap();
    let mut policy = run.policy.clone();
    policy.models = vec!["slow".into()];
    db.update_task_run(
        &run.id,
        &NewTaskRun {
            title: "Run".into(),
            policy: policy.clone(),
            steps: steps(3),
            ..Default::default()
        },
    )
    .unwrap();
    let handle = {
        let (db, id, ep) = (db.clone(), run.id.clone(), ep.clone());
        tokio::spawn(async move { run_task(db, &id, RunOptions::new(ep), flag()).await })
    };
    for _ in 0..100 {
        if db.task_steps(&run.id).unwrap()[1].status == "running" {
            break;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    assert_eq!(db.task_steps(&run.id).unwrap()[1].status, "running");
    // A live run cannot be claimed twice or deleted.
    assert!(run_task(db.clone(), &run.id, opts(&ep), flag())
        .await
        .is_err());
    assert!(db.delete_task_run(&run.id).is_err());
    handle.abort();
    let _ = handle.await;
    let recovered = recover(&db, Some(0)).unwrap();
    assert_eq!(recovered, vec![run.id.clone()]);
    let r = db.task_run(&run.id).unwrap();
    assert_eq!(r.status, "interrupted");
    let s = db.task_steps(&run.id).unwrap();
    assert_eq!(
        (s[0].status.as_str(), s[1].status.as_str()),
        ("done", "pending")
    );
    // Resume on another model than the one that hung.
    policy.models = vec!["good".into()];
    db.update_task_run(
        &run.id,
        &NewTaskRun {
            title: "Run".into(),
            policy,
            steps: steps(3),
            ..Default::default()
        },
    )
    .unwrap();
    let r = run_task(db.clone(), &run.id, opts(&ep), flag())
        .await
        .unwrap();
    assert_eq!(r.status, "completed");
    assert!(db
        .task_events(&run.id)
        .unwrap()
        .iter()
        .any(|e| e.kind == "recovered"));
}

#[tokio::test]
async fn concurrent_run_limit_and_timeout() {
    let (_d, db) = db();
    let (ep, fake) = serve().await;
    fake.slow_ms.store(60_000, Ordering::SeqCst);
    let a = new_run(&db, &["slow"], 1);
    let b = new_run(&db, &["good"], 1);
    let handle = {
        let (db, id, ep) = (db.clone(), a.id.clone(), ep.clone());
        tokio::spawn(async move { run_task(db, &id, RunOptions::new(ep), flag()).await })
    };
    for _ in 0..100 {
        if db.task_run(&a.id).unwrap().status == "running" {
            break;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    let mut o = opts(&ep);
    o.max_concurrent_runs = 1;
    let e = run_task(db.clone(), &b.id, o, flag()).await.unwrap_err();
    assert!(e.contains("taskrun.too_many_running"), "{e}");
    handle.abort();
    let _ = handle.await;
    // A request that outlives the timeout fails (and, no other model, the run fails).
    recover(&db, Some(0)).unwrap();
    let mut policy = a.policy.clone();
    policy.models = vec!["slow".into()];
    policy.timeout_secs = 5;
    policy.max_retries = 0;
    db.update_task_run(
        &a.id,
        &NewTaskRun {
            title: "Run".into(),
            policy,
            steps: steps(1),
            ..Default::default()
        },
    )
    .unwrap();
    // The 5 s floor of the timeout keeps this test short but real.
    fake.slow_ms.store(8_000, Ordering::SeqCst);
    let r = run_task(db.clone(), &a.id, opts(&ep), flag())
        .await
        .unwrap();
    assert_eq!(r.status, "failed");
    let s = &db.task_steps(&a.id).unwrap()[0];
    assert!(s
        .fallback_reason
        .as_deref()
        .unwrap()
        .contains("taskrun.timeout"));
}

#[tokio::test]
async fn stop_request_pauses_and_keeps_the_step_pending() {
    let (_d, db) = db();
    let (ep, fake) = serve().await;
    fake.slow_ms.store(60_000, Ordering::SeqCst);
    let run = new_run(&db, &["slow"], 2);
    let cancel = flag();
    let handle = {
        let (db, id, ep, c) = (db.clone(), run.id.clone(), ep.clone(), cancel.clone());
        tokio::spawn(async move { run_task(db, &id, RunOptions::new(ep), c).await })
    };
    for _ in 0..100 {
        if db.task_steps(&run.id).unwrap()[0].status == "running" {
            break;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    cancel.store(true, Ordering::SeqCst);
    let r = handle.await.unwrap().unwrap();
    assert_eq!(r.status, "paused");
    assert_eq!(db.task_steps(&run.id).unwrap()[0].status, "pending");
}

#[tokio::test]
async fn bundle_moves_a_run_with_its_files_to_another_project() {
    let dir = tempfile::tempdir().unwrap();
    let (_d, db) = db();
    let (ep, _fake) = serve().await;
    let make_project = |name: &str| {
        let folder = dir.path().join(name);
        std::fs::create_dir_all(&folder).unwrap();
        db.create_project(&ProjectInput {
            name: name.into(),
            folder: Some(folder.to_string_lossy().into()),
            ..Default::default()
        })
        .unwrap()
    };
    let src = make_project("src");
    let dst = make_project("dst");
    std::fs::create_dir_all(dir.path().join("src/data")).unwrap();
    std::fs::write(dir.path().join("src/data/a.txt"), "alpha numbers: 1 2 3").unwrap();
    let refs = resolve_file_refs(&src, &["data/a.txt".into()]).unwrap();
    assert_eq!(refs[0].path, "data/a.txt");
    assert!(resolve_file_refs(&src, &["../outside.txt".into()]).is_err());
    let run = db
        .create_task_run(&NewTaskRun {
            project_id: Some(src.id.clone()),
            kind: "analysis".into(),
            title: "Analyse".into(),
            policy: RunPolicy {
                models: vec!["good".into()],
                ..Default::default()
            },
            file_refs: refs,
            steps: steps(3),
            ..Default::default()
        })
        .unwrap();
    let mut o = opts(&ep);
    o.action = RunAction::Step;
    run_task(db.clone(), &run.id, o, flag()).await.unwrap();
    let path = dir.path().join("bundle.json");
    let report = bundle::export(&db, &run.id, &path, true).unwrap();
    assert_eq!((report.steps, report.embedded_files), (3, 1));
    let text = std::fs::read_to_string(&path).unwrap();
    assert!(!text.contains("127.0.0.1"), "endpoints stay out of bundles");
    // Import into the other project: the file is restored, one step is done, resume finishes.
    let imported = bundle::import(&db, &path, Some(&dst.id)).unwrap();
    assert_eq!(imported.restored_files, 1);
    assert_eq!(imported.files[0].state, "present");
    assert!(dir.path().join("dst/data/a.txt").is_file());
    assert_eq!(imported.run.status, "paused");
    assert_ne!(imported.run.id, run.id);
    assert!(imported.run.imported_from.is_some());
    let s = db.task_steps(&imported.run.id).unwrap();
    assert_eq!(s.iter().filter(|s| s.status == "done").count(), 1);
    let r = run_task(db.clone(), &imported.run.id, opts(&ep), flag())
        .await
        .unwrap();
    assert_eq!(r.status, "completed");
    // A changed file is reported, a bad bundle refused.
    std::fs::write(dir.path().join("dst/data/a.txt"), "edited").unwrap();
    let dst_project = db.project(&dst.id).unwrap();
    let states = file_ref_states(Some(&dst_project), &r.file_refs);
    assert_eq!(states[0].state, "changed");
    std::fs::write(dir.path().join("junk.json"), "{}").unwrap();
    assert!(bundle::import(&db, &dir.path().join("junk.json"), None).is_err());
}

#[test]
fn compatibility_notes_explain_a_model_change() {
    let (_d, db) = {
        let dir = tempfile::tempdir().unwrap();
        let db = StateDb::open(&dir.path().join("state.db")).unwrap();
        (dir, db)
    };
    let mut run = db
        .create_task_run(&NewTaskRun {
            title: "R".into(),
            steps: steps(1),
            ..Default::default()
        })
        .unwrap();
    run.last_model = Some("qwen-local".into());
    let cloud = ModelInfo {
        model: "cloud:x/m".into(),
        context: Some(1000),
        tools: Some(false),
        provider: "cloud".into(),
        ..Default::default()
    };
    let keys: Vec<String> = compat_notes(&run, 900, &cloud, true)
        .into_iter()
        .map(|n| n.message)
        .collect();
    let joined = keys.join("\n");
    for k in [
        "taskrun.compat.model_changed",
        "taskrun.compat.provider_changed",
        "taskrun.compat.cloud_data",
        "taskrun.compat.context_tight",
        "taskrun.compat.tools_unverified",
    ] {
        assert!(joined.contains(k), "missing {k}: {joined}");
    }
    let same = ModelInfo {
        model: "qwen-local".into(),
        context: Some(32768),
        tools: Some(true),
        provider: "local".into(),
        ..Default::default()
    };
    assert!(compat_notes(&run, 900, &same, true).is_empty());
}

#[test]
fn backoff_doubles_and_is_capped() {
    use exec::{backoff, cost_micro};
    assert_eq!(backoff(500, 0), Duration::from_millis(500));
    assert_eq!(backoff(500, 2), Duration::from_millis(2000));
    assert_eq!(backoff(500, 30), Duration::from_secs(30));
    assert_eq!(cost_micro(1_000_000, 0, 3.0, 15.0), 3_000_000);
}
