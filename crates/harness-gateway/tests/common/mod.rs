#![allow(dead_code)]

use harness_core::failure::FailureKind;
use harness_core::process::{LaunchConfig, ProcessManager, ServerStatus};

fn free_port() -> u16 {
    loop {
        let port = std::net::TcpListener::bind("127.0.0.1:0")
            .unwrap()
            .local_addr()
            .unwrap()
            .port();
        if port >= 1024 && ![11435, 11440, 11500].contains(&port) {
            return port;
        }
    }
}

/// A managed child must know its port before spawn. Retry only bind failures,
/// including those reported asynchronously after `start` succeeds.
pub async fn start_process(pm: &ProcessManager, mut config: LaunchConfig) -> Result<(), String> {
    for attempt in 0..5 {
        config.port = free_port();
        if let Err(error) = pm.start(config.clone()).await {
            let conflict = pm
                .last_failure()
                .is_some_and(|f| f.kind == FailureKind::PortInUse);
            if conflict && attempt < 4 {
                pm.stop().await?;
                continue;
            }
            return Err(error);
        }
        let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(30);
        loop {
            match pm.get_status() {
                ServerStatus::Running => return Ok(()),
                ServerStatus::Error(error) => {
                    let conflict = pm
                        .last_failure()
                        .is_some_and(|f| f.kind == FailureKind::PortInUse);
                    pm.stop().await?;
                    if conflict && attempt < 4 {
                        break;
                    }
                    return Err(error);
                }
                _ => {}
            }
            if tokio::time::Instant::now() >= deadline {
                pm.stop().await?;
                return Err("test child did not become ready".into());
            }
            tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        }
    }
    unreachable!()
}
