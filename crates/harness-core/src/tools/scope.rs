//! Workspace scope: file tools see only one directory tree.
//!
//! Every path a tool receives is resolved against the working directory and must end up inside
//! it *after* the operating system resolved symlinks and junctions. `..` segments are removed
//! lexically first (the tool then works on the cleaned path, never on the original text), and a
//! path that does not exist yet (a file about to be written) is checked through its deepest
//! existing ancestor, so a symlinked directory cannot be used to write outside.
//!
//! The shell tool starts in the working directory but a command can do anything its user can:
//! the shell is a [`PermissionClass::Dangerous`](super::PermissionClass) tool, not a confined one.

use std::path::{Component, Path, PathBuf};

use super::types::{ToolError, ToolErrorCode};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Workspace {
    root: PathBuf,
}

/// Strips the `\\?\` prefix `canonicalize` adds on Windows (display and child processes).
pub fn plain_path(p: &Path) -> PathBuf {
    #[cfg(windows)]
    {
        let s = p.to_string_lossy();
        if let Some(rest) = s.strip_prefix(r"\\?\UNC\") {
            return PathBuf::from(format!(r"\\{rest}"));
        }
        if let Some(rest) = s.strip_prefix(r"\\?\") {
            return PathBuf::from(rest);
        }
    }
    p.to_path_buf()
}

fn outside(raw: &str) -> ToolError {
    ToolError::new(
        ToolErrorCode::OutsideWorkspace,
        format!("path is outside the working directory: {raw}"),
    )
}

impl Workspace {
    /// The working directory `dir` (must exist; symlinks in it are resolved).
    pub fn new(dir: &Path) -> Result<Self, ToolError> {
        let root = std::fs::canonicalize(dir).map_err(|e| {
            ToolError::new(
                ToolErrorCode::NoWorkspace,
                format!(
                    "working directory is not available ({}): {e}",
                    dir.display()
                ),
            )
        })?;
        if !root.is_dir() {
            return Err(ToolError::new(
                ToolErrorCode::NoWorkspace,
                format!("working directory is not a directory: {}", dir.display()),
            ));
        }
        Ok(Self { root })
    }

    /// The canonical root (verbatim `\\?\` form on Windows; see [`Self::display_root`]).
    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn display_root(&self) -> String {
        plain_path(&self.root).to_string_lossy().replace('\\', "/")
    }

    /// `raw` joined to the root (or taken as is when absolute), with `.` and `..` removed
    /// lexically. May still point outside: the callers check.
    fn lexical(&self, raw: &str) -> Result<PathBuf, ToolError> {
        let given = Path::new(raw);
        let mut out = PathBuf::new();
        let mut absolute = false;
        for c in given.components() {
            match c {
                Component::Prefix(_) | Component::RootDir => {
                    absolute = true;
                    out.push(c.as_os_str());
                }
                Component::CurDir => {}
                Component::ParentDir => {
                    // A relative path that climbs above its start leaves the workspace.
                    if !out.pop() && !absolute {
                        return Err(outside(raw));
                    }
                }
                Component::Normal(n) => out.push(n),
            }
        }
        if absolute {
            // `C:foo` / `\foo` (a prefix or a root without the other half): rely on the
            // containment check below, which compares the result with the root.
            Ok(out)
        } else {
            let mut full = self.root.clone();
            full.extend(out.components());
            Ok(full)
        }
    }

    fn contains(&self, p: &Path) -> bool {
        p.starts_with(&self.root)
    }

    /// An existing file or directory inside the workspace (symlinks resolved).
    pub fn resolve_existing(&self, raw: &str) -> Result<PathBuf, ToolError> {
        let path = self.lexical(raw)?;
        // A path given as absolute but spelled without the verbatim prefix still has to match
        // the canonical root: canonicalize it before comparing.
        match std::fs::canonicalize(&path) {
            Ok(real) => {
                if self.contains(&real) {
                    Ok(real)
                } else {
                    Err(outside(raw))
                }
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                // A dangling symlink is not "not found": refuse to follow it anywhere.
                if std::fs::symlink_metadata(&path).is_ok() {
                    return Err(outside(raw));
                }
                // Not there at all; but say "outside" when it could never be inside.
                if !self.lexically_inside(&path) {
                    return Err(outside(raw));
                }
                Err(ToolError::new(
                    ToolErrorCode::NotFound,
                    format!("path does not exist: {raw}"),
                ))
            }
            Err(e) => Err(ToolError::new(
                ToolErrorCode::Io,
                format!("cannot access {raw}: {e}"),
            )),
        }
    }

    /// A path that may not exist yet, for writing: its deepest existing ancestor must be
    /// inside the workspace.
    pub fn resolve_new(&self, raw: &str) -> Result<PathBuf, ToolError> {
        let path = self.lexical(raw)?;
        let mut tail: Vec<std::ffi::OsString> = Vec::new();
        let mut cur = path.as_path();
        loop {
            if std::fs::symlink_metadata(cur).is_ok() {
                // Exists (a dangling symlink also "exists" here, and fails to canonicalize).
                let real = std::fs::canonicalize(cur).map_err(|_| outside(raw))?;
                if !self.contains(&real) {
                    return Err(outside(raw));
                }
                if !tail.is_empty() && !real.is_dir() {
                    return Err(ToolError::new(
                        ToolErrorCode::Failed,
                        format!("a parent of the path is a file, not a directory: {raw}"),
                    ));
                }
                let mut full = real;
                for part in tail.iter().rev() {
                    full.push(part);
                }
                return Ok(full);
            }
            match (cur.file_name(), cur.parent()) {
                (Some(name), Some(parent)) => {
                    tail.push(name.to_os_string());
                    cur = parent;
                }
                _ => return Err(outside(raw)),
            }
        }
    }

    fn lexically_inside(&self, p: &Path) -> bool {
        p.starts_with(&self.root)
            || std::fs::canonicalize(p.ancestors().find(|a| a.exists()).unwrap_or(p))
                .map(|r| self.contains(&r))
                .unwrap_or(false)
    }

    /// `path` relative to the root with `/` separators (for output).
    pub fn relative(&self, path: &Path) -> String {
        path.strip_prefix(&self.root)
            .unwrap_or(path)
            .to_string_lossy()
            .replace('\\', "/")
    }
}

#[cfg(test)]
/// Makes `link` point at `target` (a directory): a symlink, or on Windows without the
/// privilege for it a junction (`mklink /J`, needs no rights). `false` when neither works.
pub(crate) fn link_dir(link: &Path, target: &Path) -> bool {
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(target, link).is_ok()
    }
    #[cfg(windows)]
    {
        if std::os::windows::fs::symlink_dir(target, link).is_ok() {
            return true;
        }
        std::process::Command::new("cmd")
            .args(["/c", "mklink", "/J"])
            .arg(link)
            .arg(target)
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false)
    }
}

#[cfg(test)]
mod tests {
    use super::link_dir;
    use super::*;

    fn ws() -> (tempfile::TempDir, Workspace) {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("sub")).unwrap();
        std::fs::write(dir.path().join("sub").join("a.txt"), "a").unwrap();
        let w = Workspace::new(dir.path()).unwrap();
        (dir, w)
    }

    fn code(r: Result<PathBuf, ToolError>) -> ToolErrorCode {
        r.unwrap_err().code
    }

    #[test]
    fn inside_paths_resolve() {
        let (_d, w) = ws();
        let p = w.resolve_existing("sub/a.txt").unwrap();
        assert!(p.ends_with("a.txt"));
        assert_eq!(w.relative(&p), "sub/a.txt");
        // `.` and a `..` that stays inside.
        assert!(w.resolve_existing("./sub/../sub/a.txt").is_ok());
        assert_eq!(w.resolve_existing(".").unwrap(), w.root());
        // Absolute path inside.
        let abs = w.root().join("sub").join("a.txt");
        assert!(w.resolve_existing(&abs.to_string_lossy()).is_ok());
        // A new file under an existing directory, and under directories that do not exist.
        let n = w.resolve_new("sub/new/deeper/b.txt").unwrap();
        assert!(n.starts_with(w.root()) && n.ends_with("b.txt"));
    }

    #[test]
    fn missing_paths_are_not_found_not_outside() {
        let (_d, w) = ws();
        assert_eq!(
            code(w.resolve_existing("nope.txt")),
            ToolErrorCode::NotFound
        );
    }

    #[test]
    fn parent_segments_cannot_leave() {
        let (_d, w) = ws();
        for raw in ["..", "../x", "sub/../../x", "../../../../../../etc/passwd"] {
            assert_eq!(
                code(w.resolve_existing(raw)),
                ToolErrorCode::OutsideWorkspace,
                "{raw}"
            );
            assert_eq!(
                code(w.resolve_new(raw)),
                ToolErrorCode::OutsideWorkspace,
                "{raw}"
            );
        }
    }

    #[test]
    fn absolute_paths_outside_are_rejected() {
        let (_d, w) = ws();
        let other = tempfile::tempdir().unwrap();
        std::fs::write(other.path().join("secret.txt"), "s").unwrap();
        let abs = other.path().join("secret.txt");
        assert_eq!(
            code(w.resolve_existing(&abs.to_string_lossy())),
            ToolErrorCode::OutsideWorkspace
        );
        assert_eq!(
            code(w.resolve_new(&other.path().join("new.txt").to_string_lossy())),
            ToolErrorCode::OutsideWorkspace
        );
        #[cfg(windows)]
        {
            assert_eq!(
                code(w.resolve_existing(r"C:\Windows\System32")),
                ToolErrorCode::OutsideWorkspace
            );
            // Drive-relative and root-relative spellings.
            assert_eq!(
                code(w.resolve_existing("C:Windows")),
                ToolErrorCode::OutsideWorkspace
            );
            assert_eq!(
                code(w.resolve_existing(r"\Windows")),
                ToolErrorCode::OutsideWorkspace
            );
        }
        #[cfg(not(windows))]
        assert_eq!(
            code(w.resolve_existing("/etc/passwd")),
            ToolErrorCode::OutsideWorkspace
        );
    }

    #[test]
    fn symlinks_cannot_leave() {
        let (_d, w) = ws();
        let outside_dir = tempfile::tempdir().unwrap();
        std::fs::write(outside_dir.path().join("secret.txt"), "s").unwrap();
        let link = w.root().join("escape");
        assert!(
            link_dir(&link, outside_dir.path()),
            "could neither create a symlink nor a junction"
        );
        // Reading through the link.
        assert_eq!(
            code(w.resolve_existing("escape/secret.txt")),
            ToolErrorCode::OutsideWorkspace
        );
        assert_eq!(
            code(w.resolve_existing("escape")),
            ToolErrorCode::OutsideWorkspace
        );
        // Writing a new file through the link, and a new directory below it.
        assert_eq!(
            code(w.resolve_new("escape/new.txt")),
            ToolErrorCode::OutsideWorkspace
        );
        assert_eq!(
            code(w.resolve_new("escape/new/deeper.txt")),
            ToolErrorCode::OutsideWorkspace
        );
        // A link that stays inside is fine.
        let inner = w.root().join("inner");
        assert!(link_dir(&inner, &w.root().join("sub")));
        assert!(w.resolve_existing("inner/a.txt").is_ok());
    }
}
