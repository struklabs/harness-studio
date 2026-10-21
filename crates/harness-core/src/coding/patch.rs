//! Applying a unified diff to the project folder as one undoable step.
//!
//! The whole patch is checked and computed in memory first (every path through the workspace
//! rules, every hunk against the file as it is now); only then is a checkpoint taken and the
//! files written. A patch that does not fit changes nothing and leaves no checkpoint; a write
//! that fails halfway is rolled back through the checkpoint.

use std::path::Path;

use serde::Serialize;

use super::checkpoint::{Checkpoint, Store};
use super::{diff, guard};
use crate::error_key::err;

#[derive(Debug, Clone, Serialize)]
pub struct Applied {
    pub checkpoint: Checkpoint,
    /// The files the patch changed (relative paths).
    pub files: Vec<String>,
}

enum Step {
    Write(std::path::PathBuf, String),
    Delete(std::path::PathBuf),
}

pub fn apply(
    store: &Store,
    project_id: &str,
    folder: &Path,
    patch: &str,
    label: &str,
) -> Result<Applied, String> {
    let patches = diff::parse(patch)
        .map_err(|e| String::from(err("coding.patch_invalid").with("error", e)))?;
    let mut steps = Vec::new();
    let mut files = Vec::new();
    for fp in &patches {
        let path = guard::check_write_path(folder, &fp.path).map_err(|e| {
            String::from(
                err("coding.patch_path")
                    .with("path", &fp.path)
                    .with("error", e),
            )
        })?;
        let original = match std::fs::read(&path) {
            Ok(bytes) => Some(
                String::from_utf8(bytes)
                    .map_err(|_| String::from(err("coding.patch_binary").with("path", &fp.path)))?,
            ),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
            Err(e) => {
                return Err(err("coding.io_failed")
                    .with("path", path.display())
                    .with("error", e)
                    .into())
            }
        };
        let updated = diff::apply(fp, original.as_deref())
            .map_err(|e| String::from(err("coding.patch_failed").with("error", e)))?;
        steps.push(match updated {
            Some(text) => Step::Write(path, text),
            None => Step::Delete(path),
        });
        files.push(fp.path.clone());
    }
    let checkpoint = store.create(project_id, folder, label)?;
    let written = (|| -> Result<(), String> {
        for step in &steps {
            match step {
                Step::Write(path, text) => {
                    if let Some(parent) = path.parent() {
                        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
                    }
                    std::fs::write(path, text).map_err(|e| e.to_string())?;
                }
                Step::Delete(path) => std::fs::remove_file(path).map_err(|e| e.to_string())?,
            }
        }
        Ok(())
    })();
    if let Err(e) = written {
        let _ = store.undo(&checkpoint, None);
        let _ = store.delete(&checkpoint);
        return Err(err("coding.io_failed")
            .with("path", folder.display())
            .with("error", e)
            .into());
    }
    Ok(Applied { checkpoint, files })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn setup() -> (tempfile::TempDir, tempfile::TempDir, Store) {
        let (data, work) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
        std::fs::write(work.path().join("a.txt"), "one\ntwo\nthree\n").unwrap();
        std::fs::write(work.path().join("gone.txt"), "bye\n").unwrap();
        let store = Store::new(data.path().to_path_buf());
        (data, work, store)
    }

    fn read(work: &tempfile::TempDir, rel: &str) -> String {
        std::fs::read_to_string(work.path().join(rel)).unwrap()
    }

    const PATCH: &str = "\
--- a/a.txt
+++ b/a.txt
@@ -1,3 +1,3 @@
 one
-two
+TWO
 three
--- /dev/null
+++ b/sub/new.txt
@@ -0,0 +1,2 @@
+hello
+world
--- a/gone.txt
+++ /dev/null
@@ -1 +0,0 @@
-bye
";

    #[test]
    fn a_patch_edits_creates_and_deletes_and_undoes() {
        let (_data, work, store) = setup();
        let done = apply(&store, "p1", work.path(), PATCH, "test patch").unwrap();
        assert_eq!(done.files, vec!["a.txt", "sub/new.txt", "gone.txt"]);
        assert_eq!(read(&work, "a.txt"), "one\nTWO\nthree\n");
        assert_eq!(read(&work, "sub/new.txt"), "hello\nworld\n");
        assert!(!work.path().join("gone.txt").exists());
        assert_eq!(store.changes(&done.checkpoint).unwrap().len(), 3);

        store.undo(&done.checkpoint, None).unwrap();
        assert_eq!(read(&work, "a.txt"), "one\ntwo\nthree\n");
        assert_eq!(read(&work, "gone.txt"), "bye\n");
        assert!(!work.path().join("sub").exists());
    }

    #[test]
    fn a_patch_that_does_not_fit_changes_nothing() {
        let (_data, work, store) = setup();
        std::fs::write(work.path().join("a.txt"), "one\nDIFFERENT\nthree\n").unwrap();
        let e = apply(&store, "p1", work.path(), PATCH, "x").unwrap_err();
        assert!(e.starts_with("coding.patch_failed"), "{e}");
        assert_eq!(read(&work, "a.txt"), "one\nDIFFERENT\nthree\n");
        assert!(!work.path().join("sub").exists());
        assert!(
            store.list("p1").unwrap().is_empty(),
            "no checkpoint for a failed patch"
        );
    }

    #[test]
    fn patch_paths_must_stay_in_the_workspace() {
        let (_data, work, store) = setup();
        for path in ["../evil.txt", ".git/config", "sub/../../evil.txt"] {
            let patch = format!("--- /dev/null\n+++ b/{path}\n@@ -0,0 +1 @@\n+x\n");
            let e = apply(&store, "p1", work.path(), &patch, "x").unwrap_err();
            assert!(e.starts_with("coding.patch_path"), "{path}: {e}");
        }
        assert!(apply(&store, "p1", work.path(), "not a patch", "x")
            .unwrap_err()
            .starts_with("coding.patch_invalid"));
        assert!(!work.path().parent().unwrap().join("evil.txt").exists());
    }
}
