//! Calls that are running right now, by an id the client chose, so another request (the
//! Playground's Stop button) can cancel one. The table only holds the cancel tokens; the
//! rules of a call stay in [`super::ToolService`].

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use super::registry::CancelToken;

#[derive(Default)]
pub struct RunTable {
    runs: Mutex<HashMap<String, CancelToken>>,
}

/// Removes its entry from the table when dropped (the call ended, however it ended).
pub struct RunGuard {
    table: Arc<RunTable>,
    id: String,
}

impl Drop for RunGuard {
    fn drop(&mut self) {
        self.table.lock().remove(&self.id);
    }
}

impl RunTable {
    pub fn new() -> Arc<Self> {
        Arc::new(Self::default())
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, HashMap<String, CancelToken>> {
        self.runs.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Registers a call under `id`: the token to run it with and the guard that unregisters it.
    /// A second call under the same id replaces the first one's entry (ids are the client's
    /// to keep unique).
    pub fn begin(self: &Arc<Self>, id: &str) -> (CancelToken, RunGuard) {
        let token = CancelToken::new();
        self.lock().insert(id.to_string(), token.clone());
        (
            token,
            RunGuard {
                table: self.clone(),
                id: id.to_string(),
            },
        )
    }

    /// Cancels the running call `id`. `false` when no such call runs (it already ended).
    pub fn cancel(&self, id: &str) -> bool {
        match self.lock().get(id) {
            Some(t) => {
                t.cancel();
                true
            }
            None => false,
        }
    }

    pub fn running(&self) -> usize {
        self.lock().len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_registered_call_can_be_cancelled_until_it_ends() {
        let table = RunTable::new();
        let (token, guard) = table.begin("a");
        assert_eq!(table.running(), 1);
        assert!(!token.is_cancelled());
        assert!(table.cancel("a"));
        assert!(token.is_cancelled());
        assert!(!table.cancel("other"));
        drop(guard);
        assert_eq!(table.running(), 0);
        assert!(!table.cancel("a"));
    }
}
