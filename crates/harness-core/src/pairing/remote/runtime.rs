//! Desktop opt-in lifecycle. Production ticket issuance is intentionally unavailable.
use super::*;

struct Worker {
    stop: tokio::sync::watch::Sender<bool>,
    task: tokio::task::JoinHandle<()>,
}
impl Drop for Worker {
    fn drop(&mut self) {
        let _ = self.stop.send(true);
        self.task.abort();
    }
}

/// Called after data-directory initialization and whenever the Settings switch changes.
pub fn restart(tokens: TokenStore) {
    static WORKERS: OnceLock<Mutex<HashMap<PathBuf, Worker>>> = OnceLock::new();
    let mut workers = WORKERS.get_or_init(Default::default).lock().unwrap();
    workers.remove(&tokens.dir());
    if !cfg!(feature = "remote-access") || !hub_for(&tokens).remote_settings().enabled {
        return;
    }
    let dir = tokens.dir();
    let (stop, rx) = tokio::sync::watch::channel(false);
    let task = tokio::spawn(async move {
        let identity =
            tokio::task::spawn_blocking(|| noise::Identity::load(crate::secrets::store())).await;
        if let Ok(Ok(identity)) = identity {
            let _ = relay::run(
                &relay::UnavailableTickets,
                &identity,
                tokens,
                "Harness Studio",
                rx,
            )
            .await;
        }
    });
    workers.insert(dir, Worker { stop, task });
}
