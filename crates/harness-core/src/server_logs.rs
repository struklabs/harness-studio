//! Per-run llama-server log files (`logs/servers/<server id>/<run>.log`),
//! plus search and export helpers. The in-memory ring buffer keeps the live
//! tail; these files keep complete history across runs and app restarts.

use crate::paths;
use regex::RegexBuilder;
use serde::Serialize;
use std::fs::{self, File};
use std::io::{LineWriter, Read, Seek, SeekFrom};
use std::path::PathBuf;

const KEEP_RUNS: usize = 20;
const MAX_READ_BYTES: u64 = 8 * 1024 * 1024;
/// A run's log continues in a new file past this size (a server left running for weeks).
pub const MAX_RUN_LOG_BYTES: u64 = 16 * 1024 * 1024;
/// Newest files of a server kept together (older ones go even under `KEEP_RUNS`).
const MAX_TOTAL_BYTES: u64 = 64 * 1024 * 1024;

#[derive(Debug, Clone, Serialize)]
pub struct LogFileInfo {
    pub name: String,
    pub size_bytes: u64,
    /// Unix seconds.
    pub modified: u64,
}

#[derive(Debug, Clone, Serialize)]
pub struct LogMatch {
    /// 1-based line number.
    pub line: usize,
    pub text: String,
}

pub fn server_log_dir(server_id: &str) -> PathBuf {
    paths::logs_dir().join("servers").join(server_id)
}

fn valid_name(name: &str) -> bool {
    name.ends_with(".log")
        && name.len() <= 64
        && name
            .chars()
            .all(|c| c.is_ascii_digit() || c == '-' || c == '.' || c.is_ascii_lowercase())
        && !name.contains("..")
}

/// Opens a new log file for a server run (and prunes old runs).
pub fn open_run_log(server_id: &str) -> Option<LineWriter<File>> {
    let dir = server_log_dir(server_id);
    fs::create_dir_all(&dir).ok()?;
    // Sortable, filesystem-safe name: <unix seconds>-<millis>.log; a name already taken
    // (a continuation within the same millisecond) waits for the next one.
    let mut file = None;
    for _ in 0..5 {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default();
        let name = format!("{}-{:03}.log", now.as_secs(), now.subsec_millis());
        match fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(dir.join(name))
        {
            Ok(f) => {
                file = Some(f);
                break;
            }
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
                std::thread::sleep(std::time::Duration::from_millis(1));
            }
            Err(_) => return None,
        }
    }
    let file = file?;
    prune(server_id);
    Some(LineWriter::new(file))
}

fn prune(server_id: &str) {
    let files = list(server_id);
    // Newest first: keep while under both limits (the newest file always stays).
    let mut total = 0u64;
    for (i, f) in files.into_iter().enumerate() {
        total = total.saturating_add(f.size_bytes);
        if i > 0 && (i >= KEEP_RUNS || total > MAX_TOTAL_BYTES) {
            let _ = fs::remove_file(server_log_dir(server_id).join(f.name));
        }
    }
}

/// Newest first.
pub fn list(server_id: &str) -> Vec<LogFileInfo> {
    let mut out: Vec<LogFileInfo> = fs::read_dir(server_log_dir(server_id))
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|e| {
            let name = e.file_name().to_string_lossy().to_string();
            if !valid_name(&name) {
                return None;
            }
            let md = e.metadata().ok()?;
            Some(LogFileInfo {
                name,
                size_bytes: md.len(),
                modified: md
                    .modified()
                    .ok()?
                    .duration_since(std::time::UNIX_EPOCH)
                    .ok()?
                    .as_secs(),
            })
        })
        .collect();
    sort_newest(&mut out);
    out
}

fn sort_newest(files: &mut [LogFileInfo]) {
    let key = |f: &LogFileInfo| {
        f.name
            .strip_suffix(".log")
            .and_then(|s| s.split_once('-'))
            .and_then(|(epoch, millis)| {
                Some((epoch.parse::<u64>().ok()?, millis.parse::<u64>().ok()?))
            })
            .unwrap_or((f.modified, 0))
    };
    files.sort_by(|a, b| key(b).cmp(&key(a)).then_with(|| b.name.cmp(&a.name)));
}

/// Reads a run log (the last 8 MB of very large files).
pub fn read(server_id: &str, name: &str) -> Result<String, String> {
    if !valid_name(name) {
        return Err(crate::error_key::err("logs.invalid_name")
            .with("name", name)
            .into());
    }
    let path = server_log_dir(server_id).join(name);
    let read_failed = |e: std::io::Error| -> String {
        crate::error_key::err("fs.read_failed")
            .with("path", path.display())
            .with("error", e)
            .into()
    };
    let mut file = File::open(&path).map_err(read_failed)?;
    let len = file.metadata().map_err(read_failed)?.len();
    let start = len.saturating_sub(MAX_READ_BYTES);
    file.seek(SeekFrom::Start(start)).map_err(read_failed)?;
    let mut bytes = Vec::with_capacity((len - start) as usize);
    file.read_to_end(&mut bytes).map_err(read_failed)?;
    let text = String::from_utf8_lossy(&bytes).to_string();
    Ok(if start > 0 {
        // Drop the partial first line.
        text.split_once('\n')
            .map(|(_, r)| r.to_string())
            .unwrap_or(text)
    } else {
        text
    })
}

/// Case-insensitive substring (or regex) search over lines.
pub fn search<'a>(
    lines: impl Iterator<Item = &'a str>,
    query: &str,
    use_regex: bool,
    case_sensitive: bool,
    max: usize,
) -> Result<Vec<LogMatch>, String> {
    let re = if use_regex {
        Some(
            RegexBuilder::new(query)
                .case_insensitive(!case_sensitive)
                .size_limit(1 << 20)
                .build()
                .map_err(|e| {
                    String::from(crate::error_key::err("logs.invalid_regex").with("error", e))
                })?,
        )
    } else {
        None
    };
    let needle = if case_sensitive {
        query.to_string()
    } else {
        query.to_lowercase()
    };
    let mut out = Vec::new();
    for (i, line) in lines.enumerate() {
        let hit = match &re {
            Some(re) => re.is_match(line),
            None if case_sensitive => line.contains(&needle),
            None => line.to_lowercase().contains(&needle),
        };
        if hit {
            out.push(LogMatch {
                line: i + 1,
                text: line.to_string(),
            });
            if out.len() >= max {
                break;
            }
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn newest_log_uses_numeric_epoch_then_millis() {
        let mut files = [
            "1791645205-891.log",
            "1791648355-206.log",
            "999999999-999.log",
            "1791648355-9.log",
        ]
        .map(|name| LogFileInfo {
            name: name.into(),
            size_bytes: 0,
            modified: 0,
        });
        sort_newest(&mut files);
        assert_eq!(
            files.map(|f| f.name),
            [
                "1791648355-206.log",
                "1791648355-9.log",
                "1791645205-891.log",
                "999999999-999.log"
            ]
        );
    }

    #[test]
    fn test_search_modes() {
        let text = "load model\nERROR: out of memory\nwarn: slow\nerror again";
        let hits = search(text.lines(), "error", false, false, 10).unwrap();
        assert_eq!(hits.iter().map(|h| h.line).collect::<Vec<_>>(), vec![2, 4]);
        let hits = search(text.lines(), "ERROR", false, true, 10).unwrap();
        assert_eq!(hits.len(), 1);
        let hits = search(text.lines(), r"^(warn|error)\b", true, false, 10).unwrap();
        assert_eq!(hits.len(), 3);
        assert_eq!(search(text.lines(), "e", false, false, 2).unwrap().len(), 2);
        assert!(search(text.lines(), "(", true, false, 10).is_err());
    }

    #[test]
    fn test_valid_names() {
        assert!(valid_name("1790000000-123.log"));
        assert!(!valid_name("../x.log"));
        assert!(!valid_name("a/b.log"));
        assert!(!valid_name("x.txt"));
    }

    #[test]
    fn test_run_log_roundtrip_and_prune() {
        use std::io::Write;
        let id = format!("test-{}", std::process::id());
        let _ = fs::remove_dir_all(server_log_dir(&id));
        let mut w = open_run_log(&id).unwrap();
        writeln!(w, "hello").unwrap();
        drop(w);
        let files = list(&id);
        assert_eq!(files.len(), 1);
        assert_eq!(read(&id, &files[0].name).unwrap(), "hello\n");
        for i in 0..(KEEP_RUNS + 3) {
            fs::write(
                server_log_dir(&id).join(format!("100{:02}-000.log", i)),
                "x",
            )
            .unwrap();
        }
        prune(&id);
        assert_eq!(list(&id).len(), KEEP_RUNS);
        let _ = fs::remove_dir_all(server_log_dir(&id));
    }

    #[test]
    fn test_prune_keeps_newest_files_under_total_size() {
        let id = format!("size-test-{}", std::process::id());
        let dir = server_log_dir(&id);
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let big = MAX_TOTAL_BYTES / 2 - 1024;
        for name in ["10001-000.log", "10002-000.log", "10003-000.log"] {
            File::create(dir.join(name)).unwrap().set_len(big).unwrap();
        }
        prune(&id);
        let names: Vec<String> = list(&id).into_iter().map(|f| f.name).collect();
        assert_eq!(names, vec!["10003-000.log", "10002-000.log"]);
        // A single file above the limit is still kept (it is the newest run).
        File::create(dir.join("10004-000.log"))
            .unwrap()
            .set_len(MAX_TOTAL_BYTES + 1)
            .unwrap();
        prune(&id);
        let names: Vec<String> = list(&id).into_iter().map(|f| f.name).collect();
        assert_eq!(names, vec!["10004-000.log"]);
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn test_large_log_reads_only_complete_tail_lines() {
        use std::io::Write;
        let id = format!("tail-test-{}", std::process::id());
        let dir = server_log_dir(&id);
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let name = "10000-000.log";
        let mut file = File::create(dir.join(name)).unwrap();
        file.write_all(&vec![b'x'; 1024]).unwrap();
        file.write_all(&vec![b'a'; MAX_READ_BYTES as usize])
            .unwrap();
        file.write_all(b"\ncomplete-tail\n").unwrap();
        drop(file);
        let text = read(&id, name).unwrap();
        assert!(text.ends_with("complete-tail\n"));
        assert!(!text.starts_with('a'));
        let _ = fs::remove_dir_all(dir);
    }
}
