//! Checkpoints: a restore point taken before a coding run or a patch, the list of files that
//! changed since, and the undo.
//!
//! * **Git** (the project folder is the top level of a git repository and `git` runs): the
//!   working tree is written as a tree object through a *temporary index* kept in Harness's
//!   data folder, so the repository's own index, HEAD, branches and stash are never touched.
//!   A hidden ref `refs/harness/checkpoints/<id>` keeps the objects alive. Changes are the
//!   diff between that tree and the tree of the folder now; undo checks the files out of it.
//! * **Snapshot** (anything else): a copy of the folder's files in the data folder
//!   (`<data>/coding/<project>/<id>/files`), without dependency and build folders, with limits
//!   on file count and size. Changes and undo compare and restore from the copy.
//!
//! Both back the same [`Checkpoint`] / [`FileChange`] types, so a shell command that rewrites
//! files is covered as well as the edit tools. Files that are ignored (`.gitignore`) or skipped
//! (snapshot) are neither reported nor restored.

use std::collections::BTreeMap;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use serde::{Deserialize, Serialize};

use super::diff;
use crate::error_key::err;
use crate::paths;

/// Per project, older checkpoints beyond this are dropped when a new one is made.
const KEEP_PER_PROJECT: usize = 30;
const MAX_FILES: usize = 20_000;
const MAX_FILE_BYTES: u64 = 8 * 1024 * 1024;
const MAX_TOTAL_BYTES: u64 = 512 * 1024 * 1024;
/// Files listed with a diff; the rest of a huge change is counted but not shown.
const MAX_LISTED: usize = 300;
const MAX_DIFF_CHARS: usize = 200_000;
/// Folders a snapshot does not copy (dependencies and build output are not the user's edits).
const SKIPPED_DIRS: &[&str] = &[
    ".git",
    "node_modules",
    "target",
    ".venv",
    "venv",
    "__pycache__",
    ".svelte-kit",
    ".next",
    "dist",
    "build",
];

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Backend {
    Git,
    Snapshot,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Checkpoint {
    pub id: String,
    pub project_id: String,
    /// The folder the checkpoint covers (the project folder, as it was then).
    pub folder: String,
    pub backend: Backend,
    pub label: String,
    /// Milliseconds since the epoch.
    pub created_at: u64,
    /// Git: the tree of the folder at that time.
    #[serde(default)]
    pub tree: Option<String>,
    /// Files the checkpoint covers.
    pub files: usize,
    /// Everything was undone (the checkpoint stays listed).
    #[serde(default)]
    pub undone: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ChangeStatus {
    Added,
    Modified,
    Deleted,
}

#[derive(Debug, Clone, Serialize)]
pub struct FileChange {
    /// Relative to the folder, `/` separated.
    pub path: String,
    pub status: ChangeStatus,
    pub additions: usize,
    pub deletions: usize,
    pub binary: bool,
    /// Unified diff; empty for binary files and past the listing limit.
    pub diff: String,
}

/// Where checkpoints live.
#[derive(Debug, Clone)]
pub struct Store {
    root: PathBuf,
}

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

fn valid_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 80
        && id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

fn io(path: &Path, e: impl std::fmt::Display) -> String {
    err("coding.io_failed")
        .with("path", path.display())
        .with("error", e)
        .into()
}

fn truncate(mut text: String) -> String {
    if text.chars().count() > MAX_DIFF_CHARS {
        text = text.chars().take(MAX_DIFF_CHARS).collect();
        text.push_str("\n... diff truncated\n");
    }
    text
}

impl Store {
    pub fn new(root: PathBuf) -> Self {
        Self { root }
    }

    /// `<data dir>/coding`.
    pub fn shared() -> Self {
        Self::new(paths::data_root().join("coding"))
    }

    fn project_dir(&self, project_id: &str) -> Result<PathBuf, String> {
        if !valid_id(project_id) {
            return Err(err("coding.invalid_id").with("id", project_id).into());
        }
        Ok(self.root.join(project_id))
    }

    fn dir(&self, project_id: &str, id: &str) -> Result<PathBuf, String> {
        if !valid_id(id) {
            return Err(err("coding.invalid_id").with("id", id).into());
        }
        Ok(self.project_dir(project_id)?.join(id))
    }

    fn save(&self, cp: &Checkpoint) -> Result<(), String> {
        let path = self.dir(&cp.project_id, &cp.id)?.join("meta.json");
        let text = serde_json::to_string_pretty(cp).map_err(|e| io(&path, e))?;
        std::fs::write(&path, text).map_err(|e| io(&path, e))
    }

    /// The checkpoints of a project, newest first.
    pub fn list(&self, project_id: &str) -> Result<Vec<Checkpoint>, String> {
        let dir = self.project_dir(project_id)?;
        let mut out: Vec<Checkpoint> = match std::fs::read_dir(&dir) {
            Ok(rd) => rd
                .flatten()
                .filter_map(|e| std::fs::read_to_string(e.path().join("meta.json")).ok())
                .filter_map(|t| serde_json::from_str(&t).ok())
                .collect(),
            Err(_) => Vec::new(),
        };
        out.sort_by(|a, b| b.id.cmp(&a.id));
        Ok(out)
    }

    pub fn get(&self, project_id: &str, id: &str) -> Result<Checkpoint, String> {
        let path = self.dir(project_id, id)?.join("meta.json");
        let text = std::fs::read_to_string(&path)
            .map_err(|_| String::from(err("coding.checkpoint_not_found").with("id", id)))?;
        serde_json::from_str(&text).map_err(|e| io(&path, e))
    }

    /// What [`Self::create`] would use for `folder`.
    pub fn detect_backend(&self, folder: &Path) -> Backend {
        match std::fs::canonicalize(folder) {
            Ok(folder) if self.is_git_top(&folder) => Backend::Git,
            _ => Backend::Snapshot,
        }
    }

    fn is_git_top(&self, folder: &Path) -> bool {
        self.git_text(folder, None, &["rev-parse", "--show-toplevel"])
            .map(PathBuf::from)
            .is_ok_and(|top| paths::is_within(&top, folder) && paths::is_within(folder, &top))
    }

    /// Takes a checkpoint of `folder` (git when it can, a snapshot otherwise).
    pub fn create(
        &self,
        project_id: &str,
        folder: &Path,
        label: &str,
    ) -> Result<Checkpoint, String> {
        let folder = std::fs::canonicalize(folder).map_err(|_| {
            String::from(err("coding.folder_missing").with("path", folder.display()))
        })?;
        let id = format!(
            "{:013}-{}",
            now_ms(),
            &uuid::Uuid::new_v4().simple().to_string()[..8]
        );
        let dir = self.dir(project_id, &id)?;
        std::fs::create_dir_all(&dir).map_err(|e| io(&dir, e))?;
        let mut cp = Checkpoint {
            id,
            project_id: project_id.to_string(),
            folder: paths::display_path(&folder),
            backend: Backend::Snapshot,
            label: label.chars().take(200).collect(),
            created_at: now_ms(),
            tree: None,
            files: 0,
            undone: false,
        };
        let made = match self.git_create(&folder, &dir, &cp.id, &cp.label) {
            Ok(Some((tree, files))) => {
                cp.backend = Backend::Git;
                cp.tree = Some(tree);
                cp.files = files;
                Ok(())
            }
            Ok(None) => self.snapshot_create(&folder, &dir).map(|n| cp.files = n),
            Err(e) => Err(e),
        };
        if let Err(e) = made.and_then(|_| self.save(&cp)) {
            let _ = std::fs::remove_dir_all(&dir);
            return Err(e);
        }
        self.prune(project_id, &folder);
        Ok(cp)
    }

    fn prune(&self, project_id: &str, folder: &Path) {
        for old in self
            .list(project_id)
            .unwrap_or_default()
            .iter()
            .skip(KEEP_PER_PROJECT)
        {
            let _ = self.delete_with(old, folder);
        }
    }

    pub fn delete(&self, cp: &Checkpoint) -> Result<(), String> {
        self.delete_with(cp, Path::new(&cp.folder))
    }

    fn delete_with(&self, cp: &Checkpoint, folder: &Path) -> Result<(), String> {
        if cp.backend == Backend::Git {
            let _ = self.git(folder, None, &["update-ref", "-d", &ref_name(&cp.id)], None);
        }
        let dir = self.dir(&cp.project_id, &cp.id)?;
        std::fs::remove_dir_all(&dir).map_err(|e| io(&dir, e))
    }

    /// The files that differ from the checkpoint now.
    pub fn changes(&self, cp: &Checkpoint) -> Result<Vec<FileChange>, String> {
        match cp.backend {
            Backend::Git => self.git_changes(cp),
            Backend::Snapshot => self.snapshot_changes(cp),
        }
    }

    /// Puts the files back as the checkpoint had them: all changed files, or only `only`
    /// (relative paths). Returns the paths restored.
    pub fn undo(&self, cp: &Checkpoint, only: Option<&[String]>) -> Result<Vec<String>, String> {
        let restored = match cp.backend {
            Backend::Git => self.git_undo(cp, only)?,
            Backend::Snapshot => self.snapshot_undo(cp, only)?,
        };
        if only.is_none() || self.changes(cp)?.is_empty() {
            let mut done = cp.clone();
            done.undone = true;
            self.save(&done)?;
        }
        Ok(restored)
    }

    // --- git -------------------------------------------------------------------------------

    /// Runs `git` in `folder` with the repository's config unable to start programs through the
    /// usual hooks (`core.fsmonitor`, hooks path) and a private index when `index` is given.
    fn git(
        &self,
        folder: &Path,
        index: Option<&Path>,
        args: &[&str],
        stdin: Option<&[u8]>,
    ) -> Result<Vec<u8>, String> {
        let no_hooks = self.root.join("no-hooks");
        let mut cmd = Command::new("git");
        cmd.current_dir(folder)
            .args(["-c", "core.fsmonitor=false", "-c", "core.quotepath=false"])
            // Bytes in, the same bytes out: no line-ending conversion on add or restore.
            .args(["-c", "core.autocrlf=false", "-c", "core.safecrlf=false"])
            .arg("-c")
            .arg(format!("core.hooksPath={}", no_hooks.display()))
            .args(["-c", "commit.gpgsign=false", "-c", "gc.auto=0"])
            .args(args)
            .env("GIT_OPTIONAL_LOCKS", "0")
            .env("GIT_TERMINAL_PROMPT", "0")
            .env("GIT_AUTHOR_NAME", "Harness Studio")
            .env("GIT_AUTHOR_EMAIL", "harness@localhost")
            .env("GIT_COMMITTER_NAME", "Harness Studio")
            .env("GIT_COMMITTER_EMAIL", "harness@localhost")
            .stdin(if stdin.is_some() {
                Stdio::piped()
            } else {
                Stdio::null()
            })
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        if let Some(index) = index {
            cmd.env("GIT_INDEX_FILE", index);
        }
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            cmd.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
        }
        let mut child = cmd
            .spawn()
            .map_err(|e| String::from(err("coding.git_failed").with("error", e)))?;
        if let (Some(data), Some(mut pipe)) = (stdin, child.stdin.take()) {
            let _ = pipe.write_all(data);
        }
        let out = child
            .wait_with_output()
            .map_err(|e| String::from(err("coding.git_failed").with("error", e)))?;
        if out.status.success() {
            Ok(out.stdout)
        } else {
            Err(err("coding.git_failed")
                .with("error", String::from_utf8_lossy(&out.stderr).trim())
                .into())
        }
    }

    fn git_text(
        &self,
        folder: &Path,
        index: Option<&Path>,
        args: &[&str],
    ) -> Result<String, String> {
        Ok(
            String::from_utf8_lossy(&self.git(folder, index, args, None)?)
                .trim()
                .to_string(),
        )
    }

    /// `Ok(None)`: not a usable git folder (use a snapshot).
    fn git_create(
        &self,
        folder: &Path,
        dir: &Path,
        id: &str,
        label: &str,
    ) -> Result<Option<(String, usize)>, String> {
        if !self.is_git_top(folder) {
            return Ok(None);
        }
        let index = dir.join("index");
        let result = (|| {
            self.git(folder, Some(&index), &["add", "-A", "--", "."], None)?;
            let tree = self.git_text(folder, Some(&index), &["write-tree"])?;
            let files = self
                .git(folder, Some(&index), &["ls-files", "-z"], None)?
                .split(|b| *b == 0)
                .filter(|p| !p.is_empty())
                .count();
            let commit = self.git_text(
                folder,
                None,
                &[
                    "commit-tree",
                    &tree,
                    "-m",
                    &format!("Harness checkpoint: {label}"),
                ],
            )?;
            self.git(folder, None, &["update-ref", &ref_name(id), &commit], None)?;
            Ok((tree, files))
        })();
        let _ = std::fs::remove_file(&index);
        result.map(Some)
    }

    /// The tree of the folder as it is now.
    fn git_current_tree(&self, cp: &Checkpoint) -> Result<String, String> {
        let folder = Path::new(&cp.folder);
        let index = self.dir(&cp.project_id, &cp.id)?.join("current-index");
        let _ = std::fs::remove_file(&index);
        let tree = self
            .git(folder, Some(&index), &["add", "-A", "--", "."], None)
            .and_then(|_| self.git_text(folder, Some(&index), &["write-tree"]));
        let _ = std::fs::remove_file(&index);
        tree
    }

    /// `(status, path)` of the files that differ between the checkpoint and now.
    fn git_changed(
        &self,
        cp: &Checkpoint,
    ) -> Result<(String, Vec<(ChangeStatus, String)>), String> {
        let base = cp.tree.clone().unwrap_or_default();
        let now = self.git_current_tree(cp)?;
        if base == now {
            return Ok((now, Vec::new()));
        }
        let raw = self.git(
            Path::new(&cp.folder),
            None,
            &[
                "diff-tree",
                "-r",
                "-z",
                "--name-status",
                "--no-renames",
                &base,
                &now,
            ],
            None,
        )?;
        let mut parts = raw.split(|b| *b == 0).filter(|p| !p.is_empty());
        let mut out = Vec::new();
        while let (Some(status), Some(path)) = (parts.next(), parts.next()) {
            let status = match status.first() {
                Some(b'A') => ChangeStatus::Added,
                Some(b'D') => ChangeStatus::Deleted,
                _ => ChangeStatus::Modified,
            };
            out.push((status, String::from_utf8_lossy(path).into_owned()));
        }
        Ok((now, out))
    }

    fn git_changes(&self, cp: &Checkpoint) -> Result<Vec<FileChange>, String> {
        let base = cp.tree.clone().unwrap_or_default();
        let (now, changed) = self.git_changed(cp)?;
        let folder = Path::new(&cp.folder);
        let mut out = Vec::new();
        for (n, (status, path)) in changed.into_iter().enumerate() {
            let mut change = FileChange {
                path: path.clone(),
                status,
                additions: 0,
                deletions: 0,
                binary: false,
                diff: String::new(),
            };
            if n < MAX_LISTED {
                let raw = self.git(
                    folder,
                    None,
                    &[
                        "diff-tree",
                        "-p",
                        "--no-renames",
                        "--no-ext-diff",
                        "--no-textconv",
                        "--no-color",
                        &base,
                        &now,
                        "--",
                        &path,
                    ],
                    None,
                )?;
                let text = String::from_utf8_lossy(&raw).into_owned();
                change.binary =
                    text.contains("\nBinary files ") || text.contains("\nGIT binary patch");
                // Drop git's own header (`diff --git`, `index`, mode lines): the review view
                // shows `---`, `+++` and the hunks.
                let body: Vec<&str> = text
                    .lines()
                    .skip_while(|l| !l.starts_with("--- ") && !l.starts_with("Binary files"))
                    .collect();
                let mut in_hunk = false;
                for l in &body {
                    if l.starts_with("@@") {
                        in_hunk = true;
                    } else if in_hunk && l.starts_with('+') {
                        change.additions += 1;
                    } else if in_hunk && l.starts_with('-') {
                        change.deletions += 1;
                    }
                }
                if !change.binary {
                    change.diff = truncate(body.join("\n") + "\n");
                }
            }
            out.push(change);
        }
        Ok(out)
    }

    fn git_undo(&self, cp: &Checkpoint, only: Option<&[String]>) -> Result<Vec<String>, String> {
        let folder = Path::new(&cp.folder);
        let (_, changed) = self.git_changed(cp)?;
        let wanted = |p: &str| only.is_none_or(|o| o.iter().any(|x| x == p));
        let mut restore = Vec::new();
        let mut restored = Vec::new();
        for (status, path) in changed.into_iter().filter(|(_, p)| wanted(p)) {
            if status == ChangeStatus::Added {
                remove_file_in(folder, &path)?;
                restored.push(path);
            } else {
                restore.push(path);
            }
        }
        if !restore.is_empty() {
            let index = self.dir(&cp.project_id, &cp.id)?.join("undo-index");
            let _ = std::fs::remove_file(&index);
            let tree = cp.tree.clone().unwrap_or_default();
            let mut list = Vec::new();
            for p in &restore {
                list.extend_from_slice(p.as_bytes());
                list.push(0);
            }
            let result = self
                .git(folder, Some(&index), &["read-tree", &tree], None)
                .and_then(|_| {
                    self.git(
                        folder,
                        Some(&index),
                        &["checkout-index", "-f", "-z", "--stdin"],
                        Some(&list),
                    )
                });
            let _ = std::fs::remove_file(&index);
            result?;
            restored.extend(restore);
        }
        Ok(restored)
    }

    // --- snapshot --------------------------------------------------------------------------

    fn snapshot_create(&self, folder: &Path, dir: &Path) -> Result<usize, String> {
        let files_dir = dir.join("files");
        let mut manifest: BTreeMap<String, String> = BTreeMap::new();
        let mut total = 0u64;
        for (rel, abs, size) in walk(folder)? {
            total += size;
            if manifest.len() >= MAX_FILES || total > MAX_TOTAL_BYTES {
                return Err(err("coding.snapshot_too_large")
                    .num("files", MAX_FILES as u64)
                    .num("megabytes", MAX_TOTAL_BYTES / 1024 / 1024)
                    .into());
            }
            let to = files_dir.join(&rel);
            if let Some(parent) = to.parent() {
                std::fs::create_dir_all(parent).map_err(|e| io(parent, e))?;
            }
            std::fs::copy(&abs, &to).map_err(|e| io(&abs, e))?;
            manifest.insert(rel, file_hash(&abs)?);
        }
        std::fs::create_dir_all(&files_dir).map_err(|e| io(&files_dir, e))?;
        let path = dir.join("manifest.json");
        std::fs::write(&path, serde_json::to_string(&manifest).unwrap_or_default())
            .map_err(|e| io(&path, e))?;
        Ok(manifest.len())
    }

    fn manifest(&self, cp: &Checkpoint) -> Result<BTreeMap<String, String>, String> {
        let path = self.dir(&cp.project_id, &cp.id)?.join("manifest.json");
        let text = std::fs::read_to_string(&path).map_err(|e| io(&path, e))?;
        serde_json::from_str(&text).map_err(|e| io(&path, e))
    }

    fn snapshot_changed(
        &self,
        cp: &Checkpoint,
    ) -> Result<Vec<(ChangeStatus, String, PathBuf)>, String> {
        let manifest = self.manifest(cp)?;
        let folder = Path::new(&cp.folder);
        let mut seen = std::collections::BTreeSet::new();
        let mut out = Vec::new();
        for (rel, abs, _) in walk(folder)? {
            match manifest.get(&rel) {
                None => out.push((ChangeStatus::Added, rel.clone(), abs)),
                Some(hash) if *hash != file_hash(&abs)? => {
                    out.push((ChangeStatus::Modified, rel.clone(), abs))
                }
                Some(_) => {}
            }
            seen.insert(rel);
        }
        for rel in manifest.keys().filter(|r| !seen.contains(*r)) {
            out.push((ChangeStatus::Deleted, rel.clone(), folder.join(rel)));
        }
        out.sort_by(|a, b| a.1.cmp(&b.1));
        Ok(out)
    }

    fn snapshot_changes(&self, cp: &Checkpoint) -> Result<Vec<FileChange>, String> {
        let files_dir = self.dir(&cp.project_id, &cp.id)?.join("files");
        let mut out = Vec::new();
        for (n, (status, rel, abs)) in self.snapshot_changed(cp)?.into_iter().enumerate() {
            let old = (status != ChangeStatus::Added)
                .then(|| std::fs::read(files_dir.join(&rel)).ok())
                .flatten();
            let new = (status != ChangeStatus::Deleted)
                .then(|| std::fs::read(&abs).ok())
                .flatten();
            let text = |b: &Option<Vec<u8>>| b.as_ref().map(|b| String::from_utf8(b.clone()).ok());
            let (old_t, new_t) = (text(&old), text(&new));
            let binary = matches!(old_t, Some(None))
                || matches!(new_t, Some(None))
                || old.as_ref().is_some_and(|b| b.contains(&0))
                || new.as_ref().is_some_and(|b| b.contains(&0));
            let mut change = FileChange {
                path: rel.clone(),
                status,
                additions: 0,
                deletions: 0,
                binary,
                diff: String::new(),
            };
            if !binary && n < MAX_LISTED {
                let (d, stat) =
                    diff::unified(&rel, old_t.flatten().as_deref(), new_t.flatten().as_deref());
                change.additions = stat.additions;
                change.deletions = stat.deletions;
                change.diff = truncate(d);
            }
            out.push(change);
        }
        Ok(out)
    }

    fn snapshot_undo(
        &self,
        cp: &Checkpoint,
        only: Option<&[String]>,
    ) -> Result<Vec<String>, String> {
        let files_dir = self.dir(&cp.project_id, &cp.id)?.join("files");
        let folder = Path::new(&cp.folder);
        let mut restored = Vec::new();
        for (status, rel, abs) in self.snapshot_changed(cp)? {
            if only.is_some_and(|o| !o.contains(&rel)) {
                continue;
            }
            if status == ChangeStatus::Added {
                remove_file_in(folder, &rel)?;
            } else {
                if let Some(parent) = abs.parent() {
                    std::fs::create_dir_all(parent).map_err(|e| io(parent, e))?;
                }
                std::fs::copy(files_dir.join(&rel), &abs).map_err(|e| io(&abs, e))?;
            }
            restored.push(rel);
        }
        Ok(restored)
    }
}

fn ref_name(id: &str) -> String {
    format!("refs/harness/checkpoints/{id}")
}

/// Removes `rel` inside `folder` (never outside) and the folders it leaves empty.
fn remove_file_in(folder: &Path, rel: &str) -> Result<(), String> {
    let path = folder.join(rel);
    if !paths::is_within(&path, folder) {
        return Err(err("coding.outside_workspace").with("path", rel).into());
    }
    match std::fs::remove_file(&path) {
        Ok(()) => {}
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => return Err(io(&path, e)),
    }
    let mut dir = path.parent();
    while let Some(d) = dir {
        if d == folder || std::fs::remove_dir(d).is_err() {
            break;
        }
        dir = d.parent();
    }
    Ok(())
}

fn file_hash(path: &Path) -> Result<String, String> {
    use sha2::{Digest, Sha256};
    let mut file = std::fs::File::open(path).map_err(|e| io(path, e))?;
    let mut hasher = Sha256::new();
    let mut buf = [0u8; 64 * 1024];
    loop {
        let n = std::io::Read::read(&mut file, &mut buf).map_err(|e| io(path, e))?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    Ok(crate::hex::lower(&hasher.finalize()))
}

/// The files a snapshot covers: `(relative path with '/', absolute path, size)`. Symlinks,
/// skipped folders and files over the size limit are left out.
fn walk(folder: &Path) -> Result<Vec<(String, PathBuf, u64)>, String> {
    let mut out = Vec::new();
    let mut stack = vec![folder.to_path_buf()];
    while let Some(dir) = stack.pop() {
        for entry in std::fs::read_dir(&dir).map_err(|e| io(&dir, e))?.flatten() {
            let Ok(kind) = entry.file_type() else {
                continue;
            };
            let path = entry.path();
            if kind.is_symlink() {
                continue;
            }
            if kind.is_dir() {
                let name = entry.file_name();
                if !SKIPPED_DIRS.iter().any(|s| name == *s) {
                    stack.push(path);
                }
            } else if kind.is_file() {
                let size = entry.metadata().map(|m| m.len()).unwrap_or(0);
                if size > MAX_FILE_BYTES {
                    continue;
                }
                if let Ok(rel) = path.strip_prefix(folder) {
                    let rel = rel.to_string_lossy().replace('\\', "/");
                    out.push((rel, path.clone(), size));
                }
                if out.len() > MAX_FILES + 1 {
                    return Err(err("coding.snapshot_too_large")
                        .num("files", MAX_FILES as u64)
                        .num("megabytes", MAX_TOTAL_BYTES / 1024 / 1024)
                        .into());
                }
            }
        }
    }
    out.sort_by(|a, b| a.0.cmp(&b.0));
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write(root: &Path, rel: &str, text: &str) {
        let p = root.join(rel);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, text).unwrap();
    }

    fn read(root: &Path, rel: &str) -> String {
        std::fs::read_to_string(root.join(rel)).unwrap()
    }

    fn git_available() -> bool {
        Command::new("git").arg("--version").output().is_ok()
    }

    fn init_repo(dir: &Path) {
        for args in [
            vec!["init", "-q"],
            vec!["config", "user.email", "t@example.com"],
            vec!["config", "user.name", "t"],
            vec!["add", "-A"],
            vec!["commit", "-q", "-m", "init"],
        ] {
            assert!(Command::new("git")
                .args(&args)
                .current_dir(dir)
                .status()
                .unwrap()
                .success());
        }
    }

    fn exercise(folder: &Path, store: &Store, expect: Backend) {
        let cp = store.create("proj-1", folder, "before").unwrap();
        assert_eq!(cp.backend, expect);
        assert!(store.changes(&cp).unwrap().is_empty());

        write(folder, "src/a.txt", "one\nTWO\nthree\n");
        write(folder, "new/b.txt", "brand new\n");
        std::fs::remove_file(folder.join("c.txt")).unwrap();
        let changes = store.changes(&cp).unwrap();
        let by = |p: &str| changes.iter().find(|c| c.path == p).unwrap();
        assert_eq!(changes.len(), 3, "{changes:?}");
        assert_eq!(by("src/a.txt").status, ChangeStatus::Modified);
        assert_eq!(
            (by("src/a.txt").additions, by("src/a.txt").deletions),
            (1, 1)
        );
        assert!(by("src/a.txt").diff.contains("-two\n+TWO\n"));
        assert_eq!(by("new/b.txt").status, ChangeStatus::Added);
        assert_eq!(by("c.txt").status, ChangeStatus::Deleted);

        // Undo one file, then the rest.
        let first = store.undo(&cp, Some(&["src/a.txt".to_string()])).unwrap();
        assert_eq!(first, vec!["src/a.txt".to_string()]);
        assert_eq!(read(folder, "src/a.txt"), "one\ntwo\nthree\n");
        assert!(folder.join("new/b.txt").exists());
        assert!(!store.get("proj-1", &cp.id).unwrap().undone);

        store.undo(&cp, None).unwrap();
        assert!(
            !folder.join("new").exists(),
            "folders left empty are removed"
        );
        assert_eq!(read(folder, "c.txt"), "keep me\n");
        assert!(store.changes(&cp).unwrap().is_empty());
        assert!(store.get("proj-1", &cp.id).unwrap().undone);
        assert_eq!(store.list("proj-1").unwrap().len(), 1);
    }

    fn fixture(folder: &Path) {
        write(folder, "src/a.txt", "one\ntwo\nthree\n");
        write(folder, "c.txt", "keep me\n");
        write(folder, "node_modules/dep/index.js", "ignored\n");
    }

    #[test]
    fn snapshot_checkpoint_diff_and_undo() {
        let (data, work) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
        fixture(work.path());
        exercise(
            work.path(),
            &Store::new(data.path().to_path_buf()),
            Backend::Snapshot,
        );
        // Skipped folders are neither copied nor restored.
        assert_eq!(read(work.path(), "node_modules/dep/index.js"), "ignored\n");
    }

    #[test]
    fn git_checkpoint_leaves_the_repository_alone() {
        if !git_available() {
            return;
        }
        let (data, work) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
        fixture(work.path());
        write(work.path(), ".gitignore", "node_modules/\n");
        init_repo(work.path());
        let head = |d: &Path| {
            String::from_utf8(
                Command::new("git")
                    .args(["status", "--porcelain"])
                    .current_dir(d)
                    .output()
                    .unwrap()
                    .stdout,
            )
            .unwrap()
        };
        assert_eq!(head(work.path()), "");
        exercise(
            work.path(),
            &Store::new(data.path().to_path_buf()),
            Backend::Git,
        );
        // The repository's index and HEAD are as before: nothing is staged or committed.
        assert_eq!(head(work.path()), "");
    }

    #[test]
    fn a_subfolder_of_a_repository_uses_a_snapshot() {
        if !git_available() {
            return;
        }
        let (data, work) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
        write(work.path(), "sub/x.txt", "x\n");
        init_repo(work.path());
        let cp = Store::new(data.path().to_path_buf())
            .create("p", &work.path().join("sub"), "sub")
            .unwrap();
        assert_eq!(cp.backend, Backend::Snapshot);
    }

    #[test]
    fn old_checkpoints_are_pruned_and_ids_are_validated() {
        let (data, work) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
        write(work.path(), "a.txt", "a\n");
        let store = Store::new(data.path().to_path_buf());
        for _ in 0..KEEP_PER_PROJECT + 2 {
            store.create("p", work.path(), "x").unwrap();
        }
        assert_eq!(store.list("p").unwrap().len(), KEEP_PER_PROJECT);
        assert!(store.list("../evil").is_err());
        assert!(store.get("p", "..\\..\\x").is_err());
    }
}
