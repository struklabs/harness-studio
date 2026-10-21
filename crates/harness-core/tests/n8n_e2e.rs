//! Draft, edit, activate and run a workflow on a real n8n (`n8nio/n8n`) through approved
//! proposals. Ignored by default: it needs an instance and an API key with the scopes
//! `workflow:create|read|update|delete|list|publish|unpublish` (or `activate`/`deactivate`) and
//! `execution:read|list`.
//!
//! ```text
//! docker run -d --name n8n -p 5678:5678 n8nio/n8n
//! # create the owner and an API key in the UI (Settings > n8n API), then:
//! HS_N8N_URL=http://127.0.0.1:5678 HS_N8N_KEY=... \
//!   cargo test -p harness-core --test n8n_e2e -- --ignored --nocapture
//! ```

use harness_core::automations::proposals::{ProposalStatus, ProposalStore, StageWorkflow};
use harness_core::automations::{N8nClient, N8nPermission};
use serde_json::{json, Value};

fn workflow(name: &str, path: &str, greeting: &str) -> Value {
    json!({
        "name": name,
        "nodes": [
            {"name": "Webhook", "type": "n8n-nodes-base.webhook", "typeVersion": 2,
             "position": [0, 0],
             "parameters": {"path": path, "httpMethod": "POST", "responseMode": "lastNode"}},
            {"name": "Set", "type": "n8n-nodes-base.set", "typeVersion": 3.4,
             "position": [240, 0],
             "parameters": {"assignments": {"assignments": [
                 {"id": "a1", "name": "hello", "value": greeting, "type": "string"}
             ]}, "options": {}}}
        ],
        "connections": {"Webhook": {"main": [[{"node": "Set", "type": "main", "index": 0}]]}},
        "settings": {"executionOrder": "v1"}
    })
}

#[tokio::test]
#[ignore = "needs a running n8n: set HS_N8N_URL and HS_N8N_KEY"]
async fn draft_edit_activate_and_run_on_a_real_n8n() {
    let url = std::env::var("HS_N8N_URL").expect("HS_N8N_URL");
    let key = std::env::var("HS_N8N_KEY").expect("HS_N8N_KEY");
    let client = N8nClient::new(&url, &key).unwrap();
    let store = ProposalStore::new();
    let suffix = uuid::Uuid::new_v4().simple().to_string();
    let path = format!("hs-e2e-{}", &suffix[..8]);

    // 1. Draft: staged, reviewed, approved -> an inactive workflow.
    let p = store
        .stage_workflow(
            &client,
            N8nPermission::Write,
            StageWorkflow {
                workflow: Some(workflow("Harness e2e", &path, "world")),
                ..Default::default()
            },
        )
        .await
        .unwrap();
    assert!(p.diff.added > 10);
    let done = store
        .apply(&p.id, &client, N8nPermission::Write)
        .await
        .unwrap();
    assert_eq!(done.status, ProposalStatus::Applied, "{:?}", done.error);
    let wf = done.result.unwrap().workflow.unwrap();
    assert!(!wf.active);
    let id = wf.id.clone();
    println!("created {id}: {:?}", wf.webhook);

    // 2. Edit: the diff shows the one changed value.
    let p = store
        .stage_workflow(
            &client,
            N8nPermission::Write,
            StageWorkflow {
                workflow_id: Some(id.clone()),
                workflow: Some(workflow("Harness e2e", &path, "edited")),
                ..Default::default()
            },
        )
        .await
        .unwrap();
    assert_eq!((p.diff.added, p.diff.removed), (1, 1), "{:?}", p.diff);
    let done = store
        .apply(&p.id, &client, N8nPermission::Write)
        .await
        .unwrap();
    assert_eq!(done.status, ProposalStatus::Applied, "{:?}", done.error);

    // 3. Go live.
    let p = store
        .stage_workflow(
            &client,
            N8nPermission::Write,
            StageWorkflow {
                workflow_id: Some(id.clone()),
                activate: Some(true),
                ..Default::default()
            },
        )
        .await
        .unwrap();
    let done = store
        .apply(&p.id, &client, N8nPermission::Write)
        .await
        .unwrap();
    assert_eq!(done.status, ProposalStatus::Applied, "{:?}", done.error);
    assert!(done.result.unwrap().workflow.unwrap().active);

    // 4. Run it through the webhook and read the execution.
    let p = store
        .stage_run(
            &client,
            N8nPermission::Trigger,
            None,
            &id,
            Some(json!({"from": "harness"})),
            None,
        )
        .await
        .unwrap();
    let done = store
        .apply(&p.id, &client, N8nPermission::Trigger)
        .await
        .unwrap();
    assert_eq!(done.status, ProposalStatus::Applied, "{:?}", done.error);
    let result = done.result.unwrap();
    let trigger = result.trigger.unwrap();
    println!("webhook {} {}", trigger.status, trigger.body);
    assert!(
        trigger.ok && trigger.body.contains("edited"),
        "{}",
        trigger.body
    );
    let ex = result.execution.expect("n8n reported the execution");
    println!("execution {} {} {:?}", ex.id, ex.status, ex.nodes);
    assert_eq!(ex.status, "success");
    assert!(ex.nodes.iter().any(|n| n.name == "Set"));
    assert!(ex.output.unwrap().contains("edited"));

    // Clean up: deactivate and delete (the client has no delete call: the e2e does it itself).
    client.deactivate_workflow(&id).await.unwrap();
    // n8n 2.x only deletes an archived workflow; 1.x has no archive route (404) and deletes.
    let http = reqwest::Client::new();
    let base = format!("{}/api/v1/workflows/{id}", url.trim_end_matches('/'));
    let _ = http
        .post(format!("{base}/archive"))
        .header("X-N8N-API-KEY", &key)
        .send()
        .await
        .unwrap();
    let status = http
        .delete(&base)
        .header("X-N8N-API-KEY", &key)
        .send()
        .await
        .unwrap()
        .status();
    assert!(status.is_success(), "cleanup: {status}");
}
