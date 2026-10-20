//! Installed tools end to end: the shipped example package is installed into a temporary data
//! dir, registered, and run through the registry and the service against a real HTTP server on
//! localhost. The JS engine is the real one.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use serde_json::{json, Value};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

use super::*;
use crate::tools::js_engine::TRUSTED_TEST_HOSTS;
use crate::tools::registry::RunLimits;
use crate::tools::service::{LogSink, RunOptions, ToolService};
use crate::tools::{ToolInvocation, ToolOrigin, ToolPolicy, ToolSettings};

fn example_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/tools/url-title")
}

#[test]
fn builder_origin_survives_reload_and_package_replacement() {
    let root = tempfile::tempdir().unwrap();
    let registry = ToolRegistry::new();
    let info = install_with_origin(root.path(), &example_dir(), &registry, true).unwrap();
    assert!(info.user_created);
    assert!(list(root.path())[0].user_created);
    set_enabled(root.path(), &info.name, false).unwrap();
    assert!(list(root.path())[0].user_created);
    install(root.path(), &example_dir(), &registry).unwrap();
    assert!(!list(root.path())[0].user_created);
}

/// A server that answers `GET /page` with a titled page, `/untitled` without a title,
/// `/missing` with 404 and `/hop` with a redirect to another host name.
async fn serve() -> u16 {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    tokio::spawn(async move {
        loop {
            let Ok((mut sock, _)) = listener.accept().await else {
                return;
            };
            tokio::spawn(async move {
                let mut buf = vec![0u8; 4096];
                let n = sock.read(&mut buf).await.unwrap_or(0);
                let head = String::from_utf8_lossy(&buf[..n]).to_string();
                let path = head.split_whitespace().nth(1).unwrap_or("/").to_string();
                let (status, extra, body): (&str, String, String) = match path.as_str() {
                    "/page" => (
                        "200 OK",
                        String::new(),
                        "<html><head><title>\n  Hello &amp; welcome\n</title></head><body>x</body></html>".to_string(),
                    ),
                    "/untitled" => ("200 OK", String::new(), "<html><body>nothing</body></html>".to_string()),
                    "/hop" => (
                        "302 Found",
                        format!("Location: http://localhost:{}/page\r\n", sock.local_addr().unwrap().port()),
                        String::new(),
                    ),
                    _ => ("404 Not Found", String::new(), "gone".to_string()),
                };
                let reply = format!(
                    "HTTP/1.1 {status}\r\nContent-Type: text/html\r\n{extra}Content-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                );
                let _ = sock.write_all(reply.as_bytes()).await;
                let _ = sock.shutdown().await;
            });
        }
    });
    port
}

/// A copy of the example package whose allowed hosts are `hosts`.
fn example_with_hosts(into: &Path, hosts: &[&str]) -> PathBuf {
    let dst = into.join("url-title");
    std::fs::create_dir_all(&dst).unwrap();
    for f in ["index.js", "README.md"] {
        std::fs::copy(example_dir().join(f), dst.join(f)).unwrap();
    }
    let mut m: Value = serde_json::from_str(
        &std::fs::read_to_string(example_dir().join("manifest.json")).unwrap(),
    )
    .unwrap();
    m["permissions"]["network"] = json!(hosts);
    std::fs::write(dst.join("manifest.json"), m.to_string()).unwrap();
    dst
}

fn call(tool: &str, args: Value) -> ToolInvocation {
    ToolInvocation::new(tool, args, ToolOrigin::new("test"))
}

async fn run_in(registry: &ToolRegistry, ws: Option<&Path>, inv: ToolInvocation) -> ToolResult {
    let ctx = ToolCtx::new(ws.map(|w| crate::tools::Workspace::new(w).unwrap()));
    registry
        .run(
            &inv,
            &ctx,
            &RunLimits {
                timeout: Duration::from_secs(20),
                ..Default::default()
            },
        )
        .await
}

#[tokio::test]
async fn the_example_installs_registers_and_fetches_a_title() {
    let port = serve().await;
    let host = format!("127.0.0.1:{port}");
    TRUSTED_TEST_HOSTS.lock().unwrap().push(host.clone());
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("tools");
    let registry = ToolRegistry::with_builtin();

    // The shipped package as it is validates and declares what it needs.
    let shipped = inspect(&example_dir()).unwrap();
    assert_eq!(shipped.name, "url_title");
    assert_eq!(shipped.class, PermissionClass::Restricted);
    assert_eq!(shipped.network, ["example.com", "*.wikipedia.org"]);

    let src = example_with_hosts(tmp.path(), &["127.0.0.1"]);
    let info = install(&root, &src, &registry).unwrap();
    assert_eq!((info.name.as_str(), info.enabled), ("url_title", true));
    assert!(sync(&root, &registry).is_empty());
    let def = registry
        .list()
        .into_iter()
        .find(|d| d.name == "url_title")
        .unwrap();
    assert_eq!(def.source, ToolSource::Installed);
    assert_eq!(def.class, PermissionClass::Restricted);
    assert!(!def.uses_workspace);

    let ok = run_in(
        &registry,
        None,
        call("url_title", json!({"url": format!("http://{host}/page")})),
    )
    .await;
    assert_eq!(ok.error, None, "{}", ok.content);
    assert_eq!(ok.content, "Hello & welcome");
    assert_eq!(ok.network_target.as_deref(), Some(host.as_str()));

    let none = run_in(
        &registry,
        None,
        call(
            "url_title",
            json!({"url": format!("http://{host}/untitled")}),
        ),
    )
    .await;
    assert!(
        none.content.starts_with("No <title> found"),
        "{}",
        none.content
    );

    let gone = run_in(
        &registry,
        None,
        call(
            "url_title",
            json!({"url": format!("http://{host}/missing")}),
        ),
    )
    .await;
    assert_eq!(gone.error, Some(ToolErrorCode::Failed));
    assert!(gone.content.contains("HTTP 404"), "{}", gone.content);

    // A host that is not on the list is refused before any connection is made...
    let other = run_in(
        &registry,
        None,
        call(
            "url_title",
            json!({"url": format!("http://localhost:{port}/page")}),
        ),
    )
    .await;
    assert_eq!(other.error, Some(ToolErrorCode::Network));
    assert!(other.content.contains("allowed hosts"), "{}", other.content);
    let public = run_in(
        &registry,
        None,
        call("url_title", json!({"url": "https://evil.example.org/"})),
    )
    .await;
    assert_eq!(public.error, Some(ToolErrorCode::Network));
    // ...and so is a redirect from an allowed host to one that is not.
    let hop = run_in(
        &registry,
        None,
        call("url_title", json!({"url": format!("http://{host}/hop")})),
    )
    .await;
    assert_eq!(hop.error, Some(ToolErrorCode::Network));
    // The arguments are checked against the manifest's schema by the registry.
    let bad = run_in(&registry, None, call("url_title", json!({}))).await;
    assert_eq!(bad.error, Some(ToolErrorCode::InvalidArguments));

    // The policy of the service applies unchanged: restricted tools ask under the default.
    let service = ToolService::new(registry, LogSink::None);
    let url = format!("http://{host}/page");
    let opts = |policy, approved| RunOptions {
        approved,
        settings: Some(ToolSettings {
            policy,
            ..Default::default()
        }),
        ..Default::default()
    };
    let asked = service
        .run(
            call("url_title", json!({"url": url})),
            opts(ToolPolicy::AskEveryTime, false),
        )
        .await;
    assert_eq!(asked.error, Some(ToolErrorCode::ApprovalRequired));
    let allowed = service
        .run(
            call("url_title", json!({"url": url})),
            opts(ToolPolicy::AskEveryTime, true),
        )
        .await;
    assert_eq!(allowed.content, "Hello & welcome");
    let off = service
        .run(
            call("url_title", json!({"url": url})),
            opts(ToolPolicy::Off, true),
        )
        .await;
    assert_eq!(off.error, Some(ToolErrorCode::PolicyOff));
}

fn fs_package(dir: &Path, name: &str, read: &[&str], write: &[&str], code: &str) -> PathBuf {
    let p = dir.join(name);
    std::fs::create_dir_all(&p).unwrap();
    std::fs::write(
        p.join("manifest.json"),
        json!({
            "name": name,
            "version": "0.1.0",
            "description": "File access test.",
            "input_schema": {"type": "object", "properties": {"path": {"type": "string"}, "text": {"type": "string"}}, "required": ["path"]},
            "output": "text",
            "permissions": {"fs": {"read": read, "write": write}},
            "runtime": "js",
            "entry": "index.js"
        })
        .to_string(),
    )
    .unwrap();
    std::fs::write(p.join("index.js"), code).unwrap();
    p
}

#[tokio::test]
async fn file_access_stays_inside_the_granted_folders() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("tools");
    let ws = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(ws.path().join("docs")).unwrap();
    std::fs::write(ws.path().join("docs/a.txt"), "inside").unwrap();
    std::fs::write(ws.path().join("secret.txt"), "outside the grant").unwrap();
    let outer = tempfile::tempdir().unwrap();
    std::fs::write(outer.path().join("passwd"), "outside the workspace").unwrap();

    let registry = ToolRegistry::with_builtin();
    let code = "function run(a) { return a.text === undefined ? harness.fs.read(a.path) : String(harness.fs.write(a.path, a.text)); }";
    let reader = fs_package(tmp.path(), "reader", &["docs"], &[], code);
    let writer = fs_package(tmp.path(), "writer", &[], &["out"], code);
    let lister = fs_package(
        tmp.path(),
        "lister",
        &["docs"],
        &[],
        "function run(a) { return harness.fs.list(a.path).map((e) => e.name + (e.dir ? '/' : '')).join(','); }",
    );
    for p in [&reader, &writer, &lister] {
        install(&root, p, &registry).unwrap();
    }
    assert!(sync(&root, &registry).is_empty());
    let class = |n: &str| registry.list().into_iter().find(|d| d.name == n).unwrap();
    assert_eq!(class("reader").class, PermissionClass::Restricted);
    assert_eq!(class("writer").class, PermissionClass::Dangerous);
    assert!(class("reader").uses_workspace);

    let w = Some(ws.path());
    let read = |p: String| run_in(&registry, w, call("reader", json!({"path": p})));
    assert_eq!(read("docs/a.txt".into()).await.content, "inside");

    // Escapes: parent segments, absolute paths, the workspace's other files, a sibling folder.
    let outer_file = outer.path().join("passwd").to_string_lossy().to_string();
    for bad in [
        "../secret.txt".to_string(),
        "docs/../../secret.txt".to_string(),
        "docs/../secret.txt".to_string(),
        outer_file.clone(),
        "secret.txt".to_string(),
    ] {
        let r = read(bad.clone()).await;
        assert_eq!(
            r.error,
            Some(ToolErrorCode::OutsideWorkspace),
            "{bad}: {}",
            r.content
        );
        assert!(
            !r.content.contains("outside the grant")
                && !r.content.contains("outside the workspace")
        );
    }
    // A symlink or junction inside the granted folder that points out is refused as well.
    if crate::tools::scope::link_dir(&ws.path().join("docs/link"), outer.path()) {
        let r = read("docs/link/passwd".into()).await;
        assert_eq!(
            r.error,
            Some(ToolErrorCode::OutsideWorkspace),
            "{}",
            r.content
        );
    }

    // No write was granted to `reader`, no read to `writer`.
    let r = run_in(
        &registry,
        w,
        call("reader", json!({"path": "docs/new.txt", "text": "x"})),
    )
    .await;
    assert_eq!(
        r.error,
        Some(ToolErrorCode::OutsideWorkspace),
        "{}",
        r.content
    );
    assert!(!ws.path().join("docs/new.txt").exists());
    let r = run_in(&registry, w, call("writer", json!({"path": "docs/a.txt"}))).await;
    assert_eq!(
        r.error,
        Some(ToolErrorCode::OutsideWorkspace),
        "{}",
        r.content
    );

    // Writing works in the granted folder (created on demand) and nowhere else.
    let r = run_in(
        &registry,
        w,
        call("writer", json!({"path": "out/sub/n.txt", "text": "hello"})),
    )
    .await;
    assert_eq!((r.error, r.content.as_str()), (None, "5"));
    assert_eq!(
        std::fs::read_to_string(ws.path().join("out/sub/n.txt")).unwrap(),
        "hello"
    );
    for bad in ["../x.txt", "docs/x.txt", "out/../x.txt"] {
        let r = run_in(
            &registry,
            w,
            call("writer", json!({"path": bad, "text": "no"})),
        )
        .await;
        assert_eq!(
            r.error,
            Some(ToolErrorCode::OutsideWorkspace),
            "{bad}: {}",
            r.content
        );
    }
    assert!(!ws.path().join("x.txt").exists() && !ws.path().join("docs/x.txt").exists());

    let r = run_in(&registry, w, call("lister", json!({"path": "docs"}))).await;
    assert_eq!(r.content, "a.txt");
    let r = run_in(&registry, w, call("lister", json!({"path": "."}))).await;
    assert_eq!(
        r.error,
        Some(ToolErrorCode::OutsideWorkspace),
        "{}",
        r.content
    );

    // Without a workspace there is no file access at all.
    let r = run_in(
        &registry,
        None,
        call("reader", json!({"path": "docs/a.txt"})),
    )
    .await;
    assert_eq!(r.error, Some(ToolErrorCode::NoWorkspace));
}

#[tokio::test]
async fn install_remove_disable_and_replace() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("tools");
    let registry = Arc::new(ToolRegistry::with_builtin());
    let code = |v: &str| format!("function run() {{ return '{v}'; }}");
    let src = fs_package(tmp.path(), "echoer", &[], &[], &code("one"));
    install(&root, &src, &registry).unwrap();
    sync(&root, &registry);
    assert_eq!(
        run_in(&registry, None, call("echoer", json!({"path": "p"})))
            .await
            .content,
        "one"
    );
    assert_eq!(list(&root).len(), 1);

    // Disabled: listed, not registered; enabling brings it back.
    set_enabled(&root, "echoer", false).unwrap();
    sync(&root, &registry);
    assert!(registry.get("echoer").is_none());
    assert!(!list(&root)[0].enabled);
    set_enabled(&root, "echoer", true).unwrap();
    sync(&root, &registry);
    assert!(registry.get("echoer").is_some());

    // Installing again replaces the code.
    std::fs::write(src.join("index.js"), code("two")).unwrap();
    install(&root, &src, &registry).unwrap();
    sync(&root, &registry);
    assert_eq!(
        run_in(&registry, None, call("echoer", json!({"path": "p"})))
            .await
            .content,
        "two"
    );

    // A broken folder is listed with its error and does not stop the others.
    std::fs::create_dir_all(root.join("broken")).unwrap();
    std::fs::write(root.join("broken/manifest.json"), "{").unwrap();
    let skipped = sync(&root, &registry);
    assert_eq!(skipped.len(), 1);
    assert_eq!(skipped[0].0, "broken");
    assert!(list(&root)
        .iter()
        .any(|t| t.name == "broken" && t.error.is_some()));
    assert!(registry.get("echoer").is_some());

    // A package may not take the name of a built-in tool.
    let clash = fs_package(tmp.path(), "read_file", &[], &[], &code("x"));
    let e = install(&root, &clash, &registry).unwrap_err();
    assert!(e.starts_with("tools.package.name_taken"), "{e}");

    remove(&root, "echoer").unwrap();
    sync(&root, &registry);
    assert!(registry.get("echoer").is_none());
    assert!(!root.join("echoer").exists());
    assert!(remove(&root, "../x")
        .unwrap_err()
        .starts_with("tools.package.not_installed"));
    assert!(remove(&root, "echoer").is_err());
    // Built-in tools are untouched by every sync.
    assert!(registry.get("read_file").is_some());
}
