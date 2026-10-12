//! Glob matching and directory listing for the search tools.
//!
//! Patterns follow what llama-server's `file_glob_search` / `grep_search` document: a pattern
//! without `/` matches the file name at any depth; one with `/` matches the whole relative
//! path and is prefixed with `**/` unless it is already anchored (`**/...` or a leading `/`).
//! Supported syntax: `*` (not across `/`), `**` (across `/`; `**/` also matches nothing),
//! `?`, `[abc]` / `[a-z]` / `[!x]`, `{a,b}`.
//!
//! The listing skips symlinks and junctions (a link could loop or leave the workspace), the
//! usual junk directories (`.git`, `node_modules`, `target`...) and what `.gitignore` files
//! found on the way down exclude. A time budget and the call's cancellation bound the walk.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use regex::Regex;

use super::registry::CancelToken;

/// Directories a listing reports (for `type: dir`) but never descends into.
const JUNK_DIRS: &[&str] = &[
    ".git",
    ".svn",
    ".hg",
    "node_modules",
    "__pycache__",
    ".venv",
    "venv",
    "dist",
    "build",
    "target",
    ".cache",
    ".idea",
    ".vscode",
];

/// Time one listing may take.
pub const LIST_BUDGET: Duration = Duration::from_secs(15);

/// Translates a glob to an anchored regex over `/`-separated paths.
pub fn glob_to_regex(glob: &str) -> Result<Regex, String> {
    let chars: Vec<char> = glob.chars().collect();
    let mut re = String::from("^");
    let mut i = 0;
    let mut braces = 0usize;
    while i < chars.len() {
        let c = chars[i];
        match c {
            '*' => {
                if chars.get(i + 1) == Some(&'*') {
                    // `**/` = zero or more directories; a bare `**` = anything.
                    if chars.get(i + 2) == Some(&'/') {
                        re.push_str("(?:.*/)?");
                        i += 3;
                    } else {
                        re.push_str(".*");
                        i += 2;
                    }
                    continue;
                }
                re.push_str("[^/]*");
            }
            '?' => re.push_str("[^/]"),
            '[' => {
                // Copy a character class; `!` negates.
                let mut j = i + 1;
                let mut class = String::from("[");
                if chars.get(j) == Some(&'!') || chars.get(j) == Some(&'^') {
                    class.push('^');
                    j += 1;
                }
                let mut closed = false;
                let mut first = true;
                while j < chars.len() {
                    let cj = chars[j];
                    if cj == ']' && !first {
                        closed = true;
                        break;
                    }
                    if cj == '\\' || cj == '[' || cj == '&' || cj == '~' {
                        class.push('\\');
                    }
                    class.push(cj);
                    first = false;
                    j += 1;
                }
                if closed {
                    class.push(']');
                    re.push_str(&class);
                    i = j + 1;
                    continue;
                }
                re.push_str("\\[");
            }
            '{' => {
                braces += 1;
                re.push_str("(?:");
            }
            '}' if braces > 0 => {
                braces -= 1;
                re.push(')');
            }
            ',' if braces > 0 => re.push('|'),
            other => re.push_str(&regex::escape(&other.to_string())),
        }
        i += 1;
    }
    if braces > 0 {
        return Err(format!("unbalanced braces in glob: {glob}"));
    }
    re.push('$');
    Regex::new(&re).map_err(|e| format!("invalid glob {glob}: {e}"))
}

/// A compiled include/exclude pattern (see the module docs for the matching rules).
#[derive(Debug, Clone)]
pub struct PathGlob {
    basename_only: bool,
    re: Regex,
}

impl PathGlob {
    pub fn new(pattern: &str) -> Result<Self, String> {
        let basename_only = !pattern.contains('/');
        let anchored = pattern == "**" || pattern.starts_with("**/") || pattern.starts_with('/');
        let glob = if basename_only || anchored {
            pattern.trim_start_matches('/').to_string()
        } else {
            format!("**/{pattern}")
        };
        Ok(Self {
            basename_only,
            re: glob_to_regex(&glob)?,
        })
    }

    pub fn matches(&self, rel_path: &str) -> bool {
        if self.basename_only {
            let name = rel_path.rsplit('/').next().unwrap_or(rel_path);
            self.re.is_match(name)
        } else {
            self.re.is_match(rel_path)
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Files,
    Dirs,
    All,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    /// `/`-separated, relative to the listed directory.
    pub rel: String,
    pub is_dir: bool,
}

#[derive(Debug, Default)]
pub struct Listing {
    pub entries: Vec<Entry>,
    /// The walk could not see everything (time budget, unreadable directory, cancellation).
    pub truncated: bool,
}

/// One `.gitignore` line.
struct IgnoreRule {
    re: Regex,
    negate: bool,
    dir_only: bool,
}

/// The rules of one `.gitignore`, valid below `prefix` (relative to the listing's base, `""`
/// for the base itself).
struct IgnoreFile {
    prefix: String,
    rules: Vec<IgnoreRule>,
}

fn parse_gitignore(text: &str) -> Vec<IgnoreRule> {
    let mut rules = Vec::new();
    for raw in text.lines() {
        let line = raw.trim_end();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let (negate, line) = match line.strip_prefix('!') {
            Some(rest) => (true, rest),
            None => (false, line),
        };
        let (dir_only, line) = match line.strip_suffix('/') {
            Some(rest) => (true, rest),
            None => (false, line),
        };
        if line.is_empty() {
            continue;
        }
        let anchored = line.contains('/');
        let body = line.trim_start_matches('/');
        let glob = if anchored {
            body.to_string()
        } else {
            format!("**/{body}")
        };
        if let Ok(re) = glob_to_regex(&glob) {
            rules.push(IgnoreRule {
                re,
                negate,
                dir_only,
            });
        }
    }
    rules
}

fn ignored(files: &[IgnoreFile], rel: &str, is_dir: bool) -> bool {
    let mut result = false;
    for f in files {
        let sub = if f.prefix.is_empty() {
            rel
        } else if let Some(s) = rel
            .strip_prefix(f.prefix.as_str())
            .and_then(|s| s.strip_prefix('/'))
        {
            s
        } else {
            continue;
        };
        for rule in &f.rules {
            if rule.dir_only && !is_dir {
                continue;
            }
            if rule.re.is_match(sub) {
                result = !rule.negate;
            }
        }
    }
    result
}

/// Whether a directory entry is a symlink or a junction (never followed).
fn is_link(path: &Path) -> bool {
    let Ok(meta) = std::fs::symlink_metadata(path) else {
        return true;
    };
    if meta.file_type().is_symlink() {
        return true;
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        // A junction is a reparse point that std does not report as a symlink. Other reparse
        // points (cloud placeholders) are real files and directories.
        const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x400;
        if meta.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0 && meta.is_dir() {
            return true;
        }
    }
    false
}

/// Entries below `base`, `/`-relative, sorted by name within each directory.
/// `max_depth` 0 = unlimited, 1 = direct children only.
pub fn list_entries(base: &Path, max_depth: usize, kind: Kind, cancel: &CancelToken) -> Listing {
    let deadline = Instant::now() + LIST_BUDGET;
    let mut out = Listing::default();
    let mut ignore: Vec<IgnoreFile> = Vec::new();
    walk(
        base,
        "",
        1,
        max_depth,
        kind,
        deadline,
        cancel,
        &mut ignore,
        &mut out,
    );
    out
}

#[allow(clippy::too_many_arguments)]
fn walk(
    dir: &Path,
    rel_dir: &str,
    depth: usize,
    max_depth: usize,
    kind: Kind,
    deadline: Instant,
    cancel: &CancelToken,
    ignore: &mut Vec<IgnoreFile>,
    out: &mut Listing,
) {
    if cancel.is_cancelled() || Instant::now() >= deadline {
        out.truncated = true;
        return;
    }
    let read = match std::fs::read_dir(dir) {
        Ok(r) => r,
        Err(_) => {
            out.truncated = true;
            return;
        }
    };
    let mut children: Vec<(String, PathBuf)> = read
        .filter_map(|e| e.ok())
        .map(|e| (e.file_name().to_string_lossy().into_owned(), e.path()))
        .collect();
    children.sort();
    let pushed = match std::fs::read_to_string(dir.join(".gitignore")) {
        Ok(text) => {
            ignore.push(IgnoreFile {
                prefix: rel_dir.to_string(),
                rules: parse_gitignore(&text),
            });
            true
        }
        Err(_) => false,
    };
    for (name, path) in children {
        if cancel.is_cancelled() || Instant::now() >= deadline {
            out.truncated = true;
            break;
        }
        if is_link(&path) {
            continue;
        }
        let Ok(meta) = std::fs::metadata(&path) else {
            continue;
        };
        let rel = if rel_dir.is_empty() {
            name.clone()
        } else {
            format!("{rel_dir}/{name}")
        };
        if meta.is_dir() {
            if ignored(ignore, &rel, true) {
                continue;
            }
            if matches!(kind, Kind::Dirs | Kind::All) {
                out.entries.push(Entry {
                    rel: rel.clone(),
                    is_dir: true,
                });
            }
            let junk = JUNK_DIRS.iter().any(|j| j.eq_ignore_ascii_case(&name));
            if !junk && (max_depth == 0 || depth < max_depth) {
                walk(
                    &path,
                    &rel,
                    depth + 1,
                    max_depth,
                    kind,
                    deadline,
                    cancel,
                    ignore,
                    out,
                );
            }
        } else if meta.is_file()
            && matches!(kind, Kind::Files | Kind::All)
            && !ignored(ignore, &rel, false)
        {
            out.entries.push(Entry { rel, is_dir: false });
        }
    }
    if pushed {
        ignore.pop();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn m(pattern: &str, path: &str) -> bool {
        PathGlob::new(pattern).unwrap().matches(path)
    }

    #[test]
    fn glob_rules() {
        // No slash: the file name at any depth.
        assert!(m("*.rs", "main.rs"));
        assert!(m("*.rs", "src/deep/lib.rs"));
        assert!(!m("*.rs", "src/lib.rst"));
        // A slash: the whole path, auto-prefixed with `**/`.
        assert!(m("src/*.rs", "src/lib.rs"));
        assert!(m("src/*.rs", "crates/x/src/lib.rs"));
        assert!(!m("src/*.rs", "src/a/lib.rs"));
        // Anchored forms.
        assert!(m("**/*.rs", "lib.rs"));
        assert!(m("**/*.rs", "a/b/lib.rs"));
        assert!(m("/src/*.rs", "src/lib.rs"));
        assert!(!m("/src/*.rs", "crates/src/lib.rs"));
        assert!(m("**", "anything/at/all"));
        assert!(m("src/**/*.rs", "src/lib.rs"));
        assert!(m("src/**/*.rs", "src/a/b/lib.rs"));
        // `?`, classes and braces.
        assert!(m("a?.txt", "ab.txt") && !m("a?.txt", "abc.txt"));
        assert!(m("[a-c]x", "bx") && !m("[a-c]x", "dx") && m("[!a-c]x", "dx"));
        assert!(
            m("*.{rs,toml}", "Cargo.toml") && m("*.{rs,toml}", "x.rs") && !m("*.{rs,toml}", "x.md")
        );
        // Regex metacharacters in the pattern are literal.
        assert!(m("a+b(1).txt", "a+b(1).txt") && !m("a.c", "abc"));
        assert!(PathGlob::new("{a,b").is_err());
    }

    #[test]
    fn listing_skips_junk_links_and_ignored_files() {
        let d = tempfile::tempdir().unwrap();
        let r = d.path();
        for f in [
            "a.txt",
            "src/lib.rs",
            "src/deep/more.rs",
            "node_modules/pkg/index.js",
            "target/debug/x.o",
            "skipme.log",
            "keep.log",
            "docs/notes.md",
            "docs/private/s.md",
        ] {
            let p = r.join(f);
            std::fs::create_dir_all(p.parent().unwrap()).unwrap();
            std::fs::write(p, "x").unwrap();
        }
        std::fs::write(
            r.join(".gitignore"),
            "*.log\n!keep.log\ndocs/private/\n# comment\n",
        )
        .unwrap();
        let cancel = CancelToken::new();
        let l = list_entries(r, 0, Kind::Files, &cancel);
        let rels: Vec<&str> = l.entries.iter().map(|e| e.rel.as_str()).collect();
        assert_eq!(
            rels,
            [
                ".gitignore",
                "a.txt",
                "docs/notes.md",
                "keep.log",
                "src/deep/more.rs",
                "src/lib.rs"
            ]
        );
        assert!(!l.truncated);
        // Directories are reported, junk ones too, but not descended into.
        let l = list_entries(r, 0, Kind::Dirs, &cancel);
        let rels: Vec<&str> = l.entries.iter().map(|e| e.rel.as_str()).collect();
        assert!(rels.contains(&"node_modules") && !rels.contains(&"node_modules/pkg"));
        assert!(rels.contains(&"src/deep") && !rels.contains(&"docs/private"));
        // max_depth 1 = direct children.
        let l = list_entries(r, 1, Kind::All, &cancel);
        assert!(l.entries.iter().all(|e| !e.rel.contains('/')));
        // A cancelled walk says it saw only part.
        cancel.cancel();
        assert!(list_entries(r, 0, Kind::Files, &cancel).truncated);
    }
}
