//! Change notifications for front ends that show live state (the desktop UI): instead of
//! polling `list_servers` / the log buffer every second, a front end waits here and fetches
//! only after something changed. Both channels coalesce: many changes between two waits wake
//! the waiter once. Signalling costs a mutex and an atomic store; nothing is queued.
//!
//! Unlike [`crate::events`] (a broadcast of typed events for the Control API), this carries no
//! payload and never lags, and log lines (hundreds per second while a model loads) stay out of
//! the event stream.

use std::collections::BTreeSet;
use std::sync::{Mutex, OnceLock};
use tokio::sync::watch;

fn servers_tx() -> &'static watch::Sender<u64> {
    static TX: OnceLock<watch::Sender<u64>> = OnceLock::new();
    TX.get_or_init(|| watch::channel(0).0)
}

fn logs_tx() -> &'static watch::Sender<u64> {
    static TX: OnceLock<watch::Sender<u64>> = OnceLock::new();
    TX.get_or_init(|| watch::channel(0).0)
}

fn settings_tx() -> &'static watch::Sender<u64> {
    static TX: OnceLock<watch::Sender<u64>> = OnceLock::new();
    TX.get_or_init(|| watch::channel(0).0)
}

fn dirty_logs() -> &'static Mutex<BTreeSet<String>> {
    static SET: OnceLock<Mutex<BTreeSet<String>>> = OnceLock::new();
    SET.get_or_init(|| Mutex::new(BTreeSet::new()))
}

/// Something in the server list changed: an instance was added or removed, or its status,
/// failure, pending restart or connection info.
pub fn servers_changed() {
    servers_tx().send_modify(|g| *g = g.wrapping_add(1));
}

/// Wakes when [`servers_changed`] was called after the last `borrow_and_update`.
pub fn subscribe_servers() -> watch::Receiver<u64> {
    servers_tx().subscribe()
}

/// The app settings were changed by another client (the Control API); an open window reloads
/// its copy so it does not write the older one back.
pub fn settings_changed() {
    settings_tx().send_modify(|g| *g = g.wrapping_add(1));
}

/// Wakes when [`settings_changed`] was called after the last `borrow_and_update`.
pub fn subscribe_settings() -> watch::Receiver<u64> {
    settings_tx().subscribe()
}

/// Server `id` has new log lines.
pub fn log_appended(id: &str) {
    {
        let mut set = dirty_logs().lock().unwrap_or_else(|e| e.into_inner());
        if set.contains(id) {
            return; // already signalled, not collected yet
        }
        set.insert(id.to_string());
    }
    logs_tx().send_modify(|g| *g = g.wrapping_add(1));
}

/// Wakes when a server got new log lines; [`take_log_changes`] says which.
pub fn subscribe_logs() -> watch::Receiver<u64> {
    logs_tx().subscribe()
}

/// Ids of the servers with new log lines since the last call.
pub fn take_log_changes() -> Vec<String> {
    std::mem::take(&mut *dirty_logs().lock().unwrap_or_else(|e| e.into_inner()))
        .into_iter()
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[tokio::test]
    async fn server_changes_coalesce_into_one_wakeup() {
        let mut rx = subscribe_servers();
        rx.borrow_and_update();
        servers_changed();
        servers_changed();
        tokio::time::timeout(Duration::from_secs(1), rx.changed())
            .await
            .expect("woken")
            .unwrap();
        let seen = *rx.borrow_and_update();
        // One wakeup covered both calls.
        assert!(seen >= 2);
    }

    #[tokio::test]
    async fn log_changes_name_each_server_once() {
        let mut rx = subscribe_logs();
        rx.borrow_and_update();
        log_appended("changes-test-a");
        log_appended("changes-test-a");
        log_appended("changes-test-b");
        tokio::time::timeout(Duration::from_secs(1), rx.changed())
            .await
            .expect("woken")
            .unwrap();
        let ids = take_log_changes();
        assert_eq!(
            ids.iter()
                .filter(|i| i.as_str() == "changes-test-a")
                .count(),
            1
        );
        assert!(ids.iter().any(|i| i == "changes-test-b"));
    }
}
