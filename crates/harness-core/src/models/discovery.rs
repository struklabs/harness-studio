//! One inventory per root. Folder detectors retain their own depth, hidden-directory and
//! stop-at-first-model rules; only directory enumeration and impossible probes are shared.
use super::*;
use std::collections::HashSet;

#[derive(Default)]
pub(crate) struct DirectoryTree {
    children: HashMap<PathBuf, Vec<PathBuf>>,
    names: HashMap<PathBuf, HashSet<std::ffi::OsString>>,
    pub paths: Vec<PathBuf>,
}

impl DirectoryTree {
    pub fn collect(root: &Path) -> Self {
        let mut tree = Self::default();
        let mut visited = HashSet::new();
        if let Ok(c) = fs::canonicalize(root) {
            visited.insert(c);
        }
        let mut stack = vec![(root.to_path_buf(), 0usize)];
        while let Some((dir, depth)) = stack.pop() {
            let Ok(entries) = fs::read_dir(&dir) else {
                continue;
            };
            let mut children = Vec::new();
            let mut names = HashSet::new();
            for entry in entries.flatten() {
                let path = entry.path();
                let Ok(ty) = entry.file_type() else { continue };
                names.insert(entry.file_name());
                if ty.is_symlink() {
                    if fs::metadata(&path).is_ok_and(|m| m.is_file()) && is_model_extension(&path) {
                        tree.paths.push(path);
                    }
                } else if ty.is_dir() {
                    if entry.file_name() == ".cache" {
                        continue;
                    }
                    let canon = fs::canonicalize(&path).unwrap_or_else(|_| path.clone());
                    if !visited.insert(canon) {
                        continue;
                    }
                    if depth < 8 {
                        children.push(path.clone());
                        stack.push((path, depth + 1));
                    }
                } else if is_model_extension(&path) {
                    tree.paths.push(path);
                }
            }
            tree.children.insert(dir.clone(), children);
            tree.names.insert(dir, names);
        }
        tree
    }

    /// Equivalent to `tts::scan_folders` for one root, with a necessary-file prefilter.
    /// Names include symlinks: the detector, as before, decides whether a file is usable.
    pub fn scan<T, K: Eq + std::hash::Hash>(
        &self,
        root: &Path,
        required_any: &[&str],
        detect: impl Fn(&Path) -> Option<T>,
        key: impl Fn(&T) -> K,
    ) -> Vec<T> {
        let mut out = Vec::new();
        let mut seen = HashSet::new();
        let mut stack = vec![(root.to_path_buf(), 0usize)];
        while let Some((dir, depth)) = stack.pop() {
            // Windows path lookups are case insensitive, so the prefilter must be too.
            // If enumeration failed, let the original detector try its known paths.
            let candidate = self.names.get(&dir).is_none_or(|names| {
                names.iter().any(|name| {
                    required_any.iter().any(|required| {
                        if cfg!(windows) {
                            name.eq_ignore_ascii_case(required)
                        } else {
                            name == required
                        }
                    })
                })
            });
            if depth > 0 && candidate {
                if let Some(model) = detect(&dir) {
                    if seen.insert(key(&model)) {
                        out.push(model);
                    }
                    continue;
                }
            }
            if depth >= 4 {
                continue;
            }
            if let Some(children) = self.children.get(&dir) {
                for child in children {
                    if !child
                        .file_name()
                        .unwrap_or_default()
                        .to_string_lossy()
                        .starts_with('.')
                    {
                        stack.push((child.clone(), depth + 1));
                    }
                }
            }
        }
        out
    }
}
