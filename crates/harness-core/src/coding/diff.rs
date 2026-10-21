//! Unified diffs: made from two texts (the change-review view) and applied to files (patches).
//!
//! Line based and dependency free. The generator trims the common head and tail and runs an
//! LCS over the rest; a middle too large for that is shown as one replaced block, which is
//! still a correct patch, just a coarser one.

use serde::Serialize;

const CONTEXT: usize = 3;
/// `old lines * new lines` above this are diffed as one replaced block.
const MAX_LCS_CELLS: usize = 4_000_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
    Equal,
    Delete,
    Insert,
}

fn lines(text: &str) -> Vec<&str> {
    text.split_inclusive('\n').collect()
}

fn ops<'a>(old: &[&'a str], new: &[&'a str]) -> Vec<(Kind, &'a str)> {
    let head = old.iter().zip(new).take_while(|(a, b)| a == b).count();
    let tail = old[head..]
        .iter()
        .rev()
        .zip(new[head..].iter().rev())
        .take_while(|(a, b)| a == b)
        .count();
    let (om, nm) = (&old[head..old.len() - tail], &new[head..new.len() - tail]);
    let mut out: Vec<(Kind, &str)> = old[..head].iter().map(|l| (Kind::Equal, *l)).collect();
    if om.len().saturating_mul(nm.len()) > MAX_LCS_CELLS {
        out.extend(om.iter().map(|l| (Kind::Delete, *l)));
        out.extend(nm.iter().map(|l| (Kind::Insert, *l)));
    } else {
        // lcs[i][j]: length of the LCS of om[i..] and nm[j..].
        let w = nm.len() + 1;
        let mut lcs = vec![0u32; (om.len() + 1) * w];
        for i in (0..om.len()).rev() {
            for j in (0..nm.len()).rev() {
                lcs[i * w + j] = if om[i] == nm[j] {
                    lcs[(i + 1) * w + j + 1] + 1
                } else {
                    lcs[(i + 1) * w + j].max(lcs[i * w + j + 1])
                };
            }
        }
        let (mut i, mut j) = (0, 0);
        while i < om.len() && j < nm.len() {
            if om[i] == nm[j] {
                out.push((Kind::Equal, om[i]));
                i += 1;
                j += 1;
            } else if lcs[(i + 1) * w + j] >= lcs[i * w + j + 1] {
                out.push((Kind::Delete, om[i]));
                i += 1;
            } else {
                out.push((Kind::Insert, nm[j]));
                j += 1;
            }
        }
        out.extend(om[i..].iter().map(|l| (Kind::Delete, *l)));
        out.extend(nm[j..].iter().map(|l| (Kind::Insert, *l)));
    }
    out.extend(old[old.len() - tail..].iter().map(|l| (Kind::Equal, *l)));
    out
}

/// Added and removed line counts of a change.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
pub struct Stat {
    pub additions: usize,
    pub deletions: usize,
}

fn push_line(out: &mut String, prefix: char, line: &str) {
    out.push(prefix);
    out.push_str(line);
    if !line.ends_with('\n') {
        out.push_str("\n\\ No newline at end of file\n");
    }
}

/// A unified diff of `old` to `new` for the file `path` (`a/` and `b/` prefixes). `old` /
/// `new` = `None` means the file does not exist on that side. Empty when nothing changed.
pub fn unified(path: &str, old: Option<&str>, new: Option<&str>) -> (String, Stat) {
    if old == new {
        return (String::new(), Stat::default());
    }
    let (ol, nl) = (lines(old.unwrap_or("")), lines(new.unwrap_or("")));
    let all = ops(&ol, &nl);
    let mut stat = Stat::default();
    for (k, _) in &all {
        match k {
            Kind::Delete => stat.deletions += 1,
            Kind::Insert => stat.additions += 1,
            Kind::Equal => {}
        }
    }
    let mut out = String::new();
    out.push_str(&format!(
        "--- {}\n+++ {}\n",
        if old.is_some() {
            format!("a/{path}")
        } else {
            "/dev/null".into()
        },
        if new.is_some() {
            format!("b/{path}")
        } else {
            "/dev/null".into()
        }
    ));
    // Changed lines (indexes into `all`) grouped into hunks with CONTEXT lines around them.
    let changed: Vec<usize> = all
        .iter()
        .enumerate()
        .filter(|(_, (k, _))| *k != Kind::Equal)
        .map(|(i, _)| i)
        .collect();
    let mut k = 0;
    while k < changed.len() {
        let mut end = k;
        while end + 1 < changed.len() && changed[end + 1] - changed[end] <= 2 * CONTEXT + 1 {
            end += 1;
        }
        let from = changed[k].saturating_sub(CONTEXT);
        let to = (changed[end] + CONTEXT + 1).min(all.len());
        let old_start = all[..from]
            .iter()
            .filter(|(k, _)| *k != Kind::Insert)
            .count();
        let new_start = all[..from]
            .iter()
            .filter(|(k, _)| *k != Kind::Delete)
            .count();
        let old_len = all[from..to]
            .iter()
            .filter(|(k, _)| *k != Kind::Insert)
            .count();
        let new_len = all[from..to]
            .iter()
            .filter(|(k, _)| *k != Kind::Delete)
            .count();
        let start = |s: usize, len: usize| if len == 0 { s } else { s + 1 };
        out.push_str(&format!(
            "@@ -{},{} +{},{} @@\n",
            start(old_start, old_len),
            old_len,
            start(new_start, new_len),
            new_len
        ));
        for (kind, line) in &all[from..to] {
            let prefix = match kind {
                Kind::Equal => ' ',
                Kind::Delete => '-',
                Kind::Insert => '+',
            };
            push_line(&mut out, prefix, line);
        }
        k = end + 1;
    }
    (out, stat)
}

// --- applying -------------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Hunk {
    pub old_start: usize,
    /// `' '`, `'-'` or `'+'`, the line text without its newline, and whether the line ended the
    /// file without a newline.
    pub lines: Vec<(char, String, bool)>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FilePatch {
    /// The path in the patch, without `a/` / `b/`; the new path unless the file is deleted.
    pub path: String,
    pub creates: bool,
    pub deletes: bool,
    pub hunks: Vec<Hunk>,
}

fn strip_prefix(raw: &str) -> Option<String> {
    let name = raw.split('\t').next().unwrap_or(raw).trim();
    if name == "/dev/null" {
        return None;
    }
    let name = name.trim_matches('"');
    Some(
        name.strip_prefix("a/")
            .or_else(|| name.strip_prefix("b/"))
            .unwrap_or(name)
            .to_string(),
    )
}

fn range(spec: &str) -> Result<(usize, usize), String> {
    let spec = spec.trim_start_matches(['-', '+']);
    let (start, len) = spec.split_once(',').unwrap_or((spec, "1"));
    Ok((
        start
            .parse()
            .map_err(|_| format!("bad hunk range {spec}"))?,
        len.parse().map_err(|_| format!("bad hunk range {spec}"))?,
    ))
}

/// Parses a unified diff (several files allowed; `diff --git` and `index` lines are ignored).
pub fn parse(patch: &str) -> Result<Vec<FilePatch>, String> {
    let mut files = Vec::new();
    let mut it = patch.lines().peekable();
    while let Some(line) = it.next() {
        let Some(old_name) = line.strip_prefix("--- ") else {
            continue;
        };
        let Some(new_line) = it.next().and_then(|l| l.strip_prefix("+++ ")) else {
            return Err("patch: '---' line without '+++'".into());
        };
        let (old, new) = (strip_prefix(old_name), strip_prefix(new_line));
        let Some(path) = new.clone().or_else(|| old.clone()) else {
            return Err("patch: file without a name".into());
        };
        let mut file = FilePatch {
            path,
            creates: old.is_none(),
            deletes: new.is_none(),
            hunks: Vec::new(),
        };
        while let Some(next) = it.peek() {
            let Some(header) = next.strip_prefix("@@ ") else {
                break;
            };
            let header = header.to_string();
            it.next();
            let mut parts = header.split_whitespace();
            let (old_range, new_range) = (parts.next(), parts.next());
            let (Some(old_range), Some(new_range)) = (old_range, new_range) else {
                return Err(format!("patch: bad hunk header '{header}'"));
            };
            let ((old_start, mut old_left), (_, mut new_left)) =
                (range(old_range)?, range(new_range)?);
            let mut hunk = Hunk {
                old_start,
                lines: Vec::new(),
            };
            while old_left > 0 || new_left > 0 {
                let Some(l) = it.next() else {
                    return Err("patch: hunk ends early".into());
                };
                if l.starts_with('\\') {
                    if let Some(last) = hunk.lines.last_mut() {
                        last.2 = true;
                    }
                    continue;
                }
                let (tag, text) = match l.chars().next() {
                    Some(c @ (' ' | '-' | '+')) => (c, &l[1..]),
                    None => (' ', ""),
                    _ => return Err(format!("patch: unexpected line '{l}'")),
                };
                if tag != '+' {
                    old_left = old_left.saturating_sub(1);
                }
                if tag != '-' {
                    new_left = new_left.saturating_sub(1);
                }
                hunk.lines.push((tag, text.to_string(), false));
            }
            while it.peek().is_some_and(|l| l.starts_with('\\')) {
                it.next();
                if let Some(last) = hunk.lines.last_mut() {
                    last.2 = true;
                }
            }
            file.hunks.push(hunk);
        }
        if file.hunks.is_empty() {
            return Err(format!("patch: no hunks for {}", file.path));
        }
        files.push(file);
    }
    if files.is_empty() {
        return Err("patch: no file changes found".into());
    }
    Ok(files)
}

fn same(a: &str, b: &str) -> bool {
    a.trim_end_matches(['\r', '\n']) == b
}

/// Applies the hunks of `file` to `original` (`None`: the file does not exist). Context and
/// removed lines must match; a hunk may sit some lines away from where it says (earlier hunks
/// shift it), the nearest match wins. Returns the new text, `None` when the patch deletes the
/// file.
pub fn apply(file: &FilePatch, original: Option<&str>) -> Result<Option<String>, String> {
    match (original, file.creates) {
        (Some(_), true) => return Err(format!("{}: file already exists", file.path)),
        (None, false) => return Err(format!("{}: file does not exist", file.path)),
        _ => {}
    }
    let src = lines(original.unwrap_or(""));
    let crlf = src.first().is_some_and(|l| l.ends_with("\r\n"));
    let mut out: Vec<String> = Vec::new();
    let mut pos = 0usize;
    let mut shift: isize = 0;
    for (n, hunk) in file.hunks.iter().enumerate() {
        let old: Vec<&(char, String, bool)> =
            hunk.lines.iter().filter(|(t, _, _)| *t != '+').collect();
        let expected = (hunk.old_start.saturating_sub(1) as isize + shift).max(0) as usize;
        let fits = |at: usize| {
            at >= pos
                && at + old.len() <= src.len()
                && old
                    .iter()
                    .enumerate()
                    .all(|(i, (_, t, _))| same(src[at + i], t))
        };
        let found = (0..=src.len())
            .flat_map(|d| [expected.checked_sub(d), Some(expected + d)])
            .flatten()
            .find(|at| fits(*at));
        let Some(at) = found else {
            return Err(format!(
                "{}: hunk {} does not match the file (the file changed or the patch is for another version)",
                file.path,
                n + 1
            ));
        };
        shift = at as isize - (hunk.old_start.saturating_sub(1)) as isize;
        out.extend(src[pos..at].iter().map(|l| l.to_string()));
        let mut k = at;
        for (tag, text, no_eol) in &hunk.lines {
            match tag {
                ' ' => {
                    out.push(src[k].to_string());
                    k += 1;
                }
                '-' => k += 1,
                _ => {
                    let eol = if *no_eol {
                        ""
                    } else if crlf {
                        "\r\n"
                    } else {
                        "\n"
                    };
                    out.push(format!("{text}{eol}"));
                }
            }
        }
        pos = k;
    }
    out.extend(src[pos..].iter().map(|l| l.to_string()));
    if file.deletes {
        if out.iter().any(|l| !l.trim().is_empty()) {
            return Err(format!("{}: deleted file still has content", file.path));
        }
        return Ok(None);
    }
    Ok(Some(out.concat()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn round_trip(old: Option<&str>, new: Option<&str>) {
        let (text, _) = unified("f.txt", old, new);
        let parsed = parse(&text).unwrap();
        assert_eq!(parsed.len(), 1);
        assert_eq!(apply(&parsed[0], old).unwrap().as_deref(), new, "{text}");
    }

    #[test]
    fn generated_diffs_apply_back() {
        round_trip(Some("a\nb\nc\n"), Some("a\nB\nc\nd\n"));
        round_trip(Some("a\nb\nc"), Some("a\nb\nc\n"));
        round_trip(
            Some("1\n2\n3\n4\n5\n6\n7\n8\n9\n10\n"),
            Some("1\n2\n3\n4\nX\n6\n7\n8\n9\nY\n"),
        );
        round_trip(None, Some("new file\nsecond\n"));
        round_trip(Some("gone\n"), None);
    }

    #[test]
    fn counts_additions_and_deletions() {
        let (text, stat) = unified("f", Some("a\nb\n"), Some("a\nc\nd\n"));
        assert_eq!(
            stat,
            Stat {
                additions: 2,
                deletions: 1
            }
        );
        assert!(text.starts_with("--- a/f\n+++ b/f\n@@ -1,2 +1,3 @@\n a\n-b\n+c\n+d\n"));
        assert_eq!(unified("f", Some("a"), Some("a")).0, "");
    }

    #[test]
    fn hunks_follow_the_text_when_lines_moved() {
        let old = "a\nb\nc\nd\ne\nf\n";
        let (text, _) = unified("f", Some(old), Some("a\nb\nc\nD\ne\nf\n"));
        let parsed = parse(&text).unwrap();
        // The file gained two lines at the top since the patch was made.
        let moved = format!("x\ny\n{old}");
        let out = apply(&parsed[0], Some(&moved)).unwrap().unwrap();
        assert_eq!(out, "x\ny\na\nb\nc\nD\ne\nf\n");
    }

    #[test]
    fn a_mismatch_is_an_error_and_crlf_is_kept() {
        let (text, _) = unified("f", Some("a\nb\n"), Some("a\nc\n"));
        let parsed = parse(&text).unwrap();
        assert!(apply(&parsed[0], Some("a\nz\n"))
            .unwrap_err()
            .contains("hunk 1"));
        assert_eq!(
            apply(&parsed[0], Some("a\r\nb\r\n")).unwrap().unwrap(),
            "a\r\nc\r\n"
        );
        assert!(apply(&parsed[0], None)
            .unwrap_err()
            .contains("does not exist"));
    }

    #[test]
    fn rejects_garbage() {
        assert!(parse("hello").is_err());
        assert!(parse("--- a/x\n+++ b/x\n").is_err());
    }
}
