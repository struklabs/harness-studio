//! Checkpoints for agent file changes: before `write_file` or `edit_file` changes a file, the
//! file's previous content is kept, so the user can undo what an agent did.
//!
//! * One [`Checkpoint`] per changed file per call, listed newest first, grouped by the run
//!   (the conversation id, else the client name) so a whole run can be undone at once.
//! * The previous content lives in `<data dir>/checkpoints/blobs/<id>`, the list in
//!   `checkpoints/index.json`. A file larger than [`MAX_BLOB_BYTES`] is recorded but not kept
//!   (`restorable: false`); only the newest [`MAX_ENTRIES`] checkpoints are kept.
//! * Undo is safe by default: if the file changed again after the agent's write (the user or
//!   another run edited it), the undo is refused (`checkpoint.changed_since`) unless the
//!   caller forces it. A file the agent created is deleted by the undo.
//!
//! Nothing here is reachable from a tool call: a model cannot undo, prune or edit checkpoints.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::error_key::err;
use crate::store::JsonStore;

/// Largest previous content that is kept.
pub const MAX_BLOB_BYTES: u64 = 8 * 1024 * 1024;
/// Checkpoints kept; the oldest are dropped (with their blobs) beyond this.
pub const MAX_ENTRIES: usize = 200;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct Checkpoint {
    pub id: String,
    /// The run: conversation id, else the client name.
    pub group: String,
    /// `write_file` or `edit_file`.
    pub tool: String,
    /// Absolute path of the changed file.
    pub path: String,
    /// The file did not exist before the change (undo deletes it).
    pub created: bool,
    /// Size of the previous content in bytes.
    pub bytes_before: u64,
    /// The previous content was kept and can be restored.
    pub restorable: bool,
    pub at_ms: u64,
    /// The change was undone.
    pub undone: bool,
    /// SHA-256 of the file as the agent left it (undo checks it).
    #[serde(default)]
    pub after_sha256: String,
}

#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(default)]
struct Index {
    entries: Vec<Checkpoint>,
}

pub struct Checkpoints {
    dir: PathBuf,
}

/// A change that is about to happen: the previous content is already kept.
pub struct PendingChange {
    store: Checkpoints,
    entry: Checkpoint,
}

impl PendingChange {
    /// [`Checkpoints::begin`] on the user's checkpoints.
    pub fn begin(group: &str, tool: &str, path: &Path) -> Option<Self> {
        Checkpoints::shared().begin(group, tool, path)
    }
}

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

fn sha(bytes: &[u8]) -> String {
    crate::hex::lower(&Sha256::digest(bytes))
}

impl Checkpoints {
    /// The user's checkpoints: `checkpoints/` in the data directory.
    pub fn shared() -> Self {
        Self::at(crate::paths::data_root().join("checkpoints"))
    }

    pub fn at(dir: impl Into<PathBuf>) -> Self {
        Self { dir: dir.into() }
    }

    fn index(&self) -> JsonStore {
        JsonStore::sensitive(self.dir.join("index.json"))
    }

    fn blob(&self, id: &str) -> PathBuf {
        self.dir.join("blobs").join(id)
    }

    /// Keeps the current content of `path` (if any) before a change. `None` when the
    /// checkpoint could not be prepared: the caller still makes its change (a failing backup
    /// must not turn a normal write into an error), but nothing can be undone.
    pub fn begin(&self, group: &str, tool: &str, path: &Path) -> Option<PendingChange> {
        let id = uuid::Uuid::new_v4().simple().to_string();
        let (created, bytes_before, restorable) = match std::fs::metadata(path) {
            Err(_) => (true, 0, true),
            Ok(m) if !m.is_file() => return None,
            Ok(m) if m.len() > MAX_BLOB_BYTES => (false, m.len(), false),
            Ok(m) => {
                let old = std::fs::read(path).ok()?;
                std::fs::create_dir_all(self.dir.join("blobs")).ok()?;
                std::fs::write(self.blob(&id), &old).ok()?;
                (false, m.len(), true)
            }
        };
        Some(PendingChange {
            store: Checkpoints {
                dir: self.dir.clone(),
            },
            entry: Checkpoint {
                id,
                group: group.to_string(),
                tool: tool.to_string(),
                path: path.to_string_lossy().into_owned(),
                created,
                bytes_before,
                restorable,
                at_ms: now_ms(),
                undone: false,
                after_sha256: String::new(),
            },
        })
    }

    /// Checkpoints, newest first.
    pub fn list(&self) -> Vec<Checkpoint> {
        let mut v = self.index().load_or_default::<Index>().entries;
        v.sort_by_key(|c| std::cmp::Reverse(c.at_ms));
        v
    }

    fn find(&self, id: &str) -> Result<Checkpoint, String> {
        self.list()
            .into_iter()
            .find(|c| c.id == id)
            .ok_or_else(|| err("checkpoint.not_found").with("id", id).into())
    }

    /// Undoes one change. `force`: restore even if the file changed after the agent's write.
    pub fn undo(&self, id: &str, force: bool) -> Result<Checkpoint, String> {
        let cp = self.find(id)?;
        if cp.undone {
            return Err(err("checkpoint.already_undone")
                .with("path", &cp.path)
                .into());
        }
        if !cp.restorable {
            return Err(err("checkpoint.not_restorable")
                .with("path", &cp.path)
                .into());
        }
        let path = Path::new(&cp.path);
        if !force {
            let now = std::fs::read(path).ok().map(|b| sha(&b));
            let expected = (!cp.after_sha256.is_empty()).then_some(cp.after_sha256.as_str());
            // A file that vanished is fine to restore; one that differs is not.
            if let (Some(now), Some(expected)) = (&now, expected) {
                if now != expected {
                    return Err(err("checkpoint.changed_since")
                        .with("path", &cp.path)
                        .into());
                }
            }
        }
        if cp.created {
            match std::fs::remove_file(path) {
                Ok(()) => {}
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                Err(e) => {
                    return Err(err("checkpoint.io")
                        .with("path", &cp.path)
                        .with("error", e)
                        .into())
                }
            }
        } else {
            let old = std::fs::read(self.blob(&cp.id)).map_err(|_| {
                String::from(err("checkpoint.not_restorable").with("path", &cp.path))
            })?;
            if let Some(parent) = path.parent() {
                let _ = std::fs::create_dir_all(parent);
            }
            std::fs::write(path, old).map_err(|e| {
                String::from(err("checkpoint.io").with("path", &cp.path).with("error", e))
            })?;
        }
        let id = cp.id.clone();
        self.index().update::<Index, _>(|i| {
            if let Some(e) = i.entries.iter_mut().find(|e| e.id == id) {
                e.undone = true;
            }
            Ok(())
        })?;
        Ok(Checkpoint { undone: true, ..cp })
    }

    /// Undoes every not-yet-undone change of a run, newest first. Stops at the first refusal
    /// and reports it; what was undone before it stays undone.
    pub fn undo_group(&self, group: &str, force: bool) -> Result<Vec<Checkpoint>, String> {
        let mut done = Vec::new();
        for cp in self
            .list()
            .into_iter()
            .filter(|c| c.group == group && !c.undone)
        {
            done.push(self.undo(&cp.id, force)?);
        }
        Ok(done)
    }

    /// Deletes every checkpoint and its kept content.
    pub fn clear(&self) -> Result<(), String> {
        self.index().save(&Index::default())?;
        let _ = std::fs::remove_dir_all(self.dir.join("blobs"));
        Ok(())
    }
}

impl PendingChange {
    /// Records the change as made; `after` is the file's new content.
    pub fn commit(mut self, after: &[u8]) {
        self.entry.after_sha256 = sha(after);
        let entry = self.entry;
        let store = &self.store;
        let dropped = store.index().update::<Index, _>(|i| {
            i.entries.push(entry);
            i.entries.sort_by_key(|c| std::cmp::Reverse(c.at_ms));
            Ok(i.entries.split_off(i.entries.len().min(MAX_ENTRIES)))
        });
        if let Ok(old) = dropped {
            for e in old {
                let _ = std::fs::remove_file(store.blob(&e.id));
            }
        }
    }

    /// The change failed: the kept content is dropped again.
    pub fn abandon(self) {
        let _ = std::fs::remove_file(self.store.blob(&self.entry.id));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn change(cp: &Checkpoints, group: &str, path: &Path, new: &str) {
        let p = cp.begin(group, "write_file", path).unwrap();
        std::fs::write(path, new).unwrap();
        p.commit(new.as_bytes());
    }

    #[test]
    fn overwrite_is_undone_to_the_previous_content() {
        let d = tempfile::tempdir().unwrap();
        let cp = Checkpoints::at(d.path().join("cp"));
        let f = d.path().join("a.txt");
        std::fs::write(&f, "old").unwrap();
        change(&cp, "run1", &f, "new");
        let list = cp.list();
        assert_eq!(list.len(), 1);
        assert!(!list[0].created && list[0].restorable && !list[0].undone);
        cp.undo(&list[0].id, false).unwrap();
        assert_eq!(std::fs::read_to_string(&f).unwrap(), "old");
        assert!(cp.list()[0].undone);
        // Twice is refused.
        let e = cp.undo(&list[0].id, false).unwrap_err();
        assert!(e.starts_with("checkpoint.already_undone"), "{e}");
    }

    #[test]
    fn a_created_file_is_deleted_by_the_undo() {
        let d = tempfile::tempdir().unwrap();
        let cp = Checkpoints::at(d.path().join("cp"));
        let f = d.path().join("new.txt");
        change(&cp, "run1", &f, "hello");
        assert!(cp.list()[0].created);
        cp.undo(&cp.list()[0].id, false).unwrap();
        assert!(!f.exists());
    }

    #[test]
    fn undo_refuses_a_file_changed_since_unless_forced() {
        let d = tempfile::tempdir().unwrap();
        let cp = Checkpoints::at(d.path().join("cp"));
        let f = d.path().join("a.txt");
        std::fs::write(&f, "old").unwrap();
        change(&cp, "run1", &f, "agent");
        std::fs::write(&f, "user edit").unwrap();
        let id = cp.list()[0].id.clone();
        let e = cp.undo(&id, false).unwrap_err();
        assert!(e.starts_with("checkpoint.changed_since"), "{e}");
        assert_eq!(std::fs::read_to_string(&f).unwrap(), "user edit");
        cp.undo(&id, true).unwrap();
        assert_eq!(std::fs::read_to_string(&f).unwrap(), "old");
    }

    #[test]
    fn a_run_is_undone_newest_first_and_other_runs_are_left_alone() {
        let d = tempfile::tempdir().unwrap();
        let cp = Checkpoints::at(d.path().join("cp"));
        let f = d.path().join("a.txt");
        let g = d.path().join("b.txt");
        std::fs::write(&f, "v0").unwrap();
        std::fs::write(&g, "g0").unwrap();
        change(&cp, "run1", &f, "v1");
        std::thread::sleep(std::time::Duration::from_millis(3));
        change(&cp, "run1", &f, "v2");
        std::thread::sleep(std::time::Duration::from_millis(3));
        change(&cp, "run2", &g, "g1");
        let done = cp.undo_group("run1", false).unwrap();
        assert_eq!(done.len(), 2);
        assert_eq!(std::fs::read_to_string(&f).unwrap(), "v0");
        assert_eq!(std::fs::read_to_string(&g).unwrap(), "g1");
    }

    #[test]
    fn unknown_ids_and_abandoned_changes_leave_nothing() {
        let d = tempfile::tempdir().unwrap();
        let cp = Checkpoints::at(d.path().join("cp"));
        let e = cp.undo("nope", false).unwrap_err();
        assert!(e.starts_with("checkpoint.not_found"), "{e}");
        let f = d.path().join("a.txt");
        std::fs::write(&f, "x").unwrap();
        cp.begin("g", "write_file", &f).unwrap().abandon();
        assert!(cp.list().is_empty());
        let blobs = std::fs::read_dir(d.path().join("cp/blobs"))
            .unwrap()
            .count();
        assert_eq!(blobs, 0);
    }

    #[test]
    fn only_the_newest_entries_are_kept() {
        let d = tempfile::tempdir().unwrap();
        let cp = Checkpoints::at(d.path().join("cp"));
        let f = d.path().join("a.txt");
        std::fs::write(&f, "0").unwrap();
        for i in 0..(MAX_ENTRIES + 5) {
            change(&cp, "g", &f, &i.to_string());
        }
        assert_eq!(cp.list().len(), MAX_ENTRIES);
        let blobs = std::fs::read_dir(d.path().join("cp/blobs"))
            .unwrap()
            .count();
        assert_eq!(blobs, MAX_ENTRIES);
    }
}
