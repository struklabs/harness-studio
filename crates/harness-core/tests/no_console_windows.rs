//! Guard: no child process may open a console window on Windows.
//!
//! Every production spawn goes through `harness_core::process::hidden_command` /
//! `hidden_tokio_command` (or `quiet_command`), which set `CREATE_NO_WINDOW`. This test scans
//! the non-test sources of every crate and of `src-tauri` and fails on a bare
//! `Command::new(`. A deliberate exception carries `// visible-console: <why>` (a console or
//! GUI app the user asked for) or `// hidden: <why>` (the flag is applied another way) on the
//! same line or the line above.

use std::fs;
use std::path::{Path, PathBuf};

fn rust_files(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
            if matches!(name, "tests" | "target" | "node_modules" | "benches") {
                continue;
            }
            rust_files(&path, out);
        } else if path.extension().is_some_and(|e| e == "rs") {
            out.push(path);
        }
    }
}

fn is_test_file(path: &Path) -> bool {
    let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
    name == "tests.rs" || name == "e2e.rs" || name.ends_with("_tests.rs")
}

#[test]
fn every_spawn_hides_the_console_window() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("workspace root");
    let mut files = Vec::new();
    if let Ok(crates) = fs::read_dir(root.join("crates")) {
        for c in crates.flatten() {
            rust_files(&c.path().join("src"), &mut files);
        }
    }
    rust_files(&root.join("src-tauri").join("src"), &mut files);
    assert!(
        !files.is_empty(),
        "no sources found under {}",
        root.display()
    );

    let mut offenders = Vec::new();
    for file in files.iter().filter(|f| !is_test_file(f)) {
        let Ok(text) = fs::read_to_string(file) else {
            continue;
        };
        let lines: Vec<&str> = text.lines().collect();
        // End line (exclusive) of the test module being skipped, if any.
        let mut skip_until = 0;
        for (i, line) in lines.iter().enumerate() {
            if i < skip_until {
                continue;
            }
            // `#[cfg(test)] mod ...` is test code: to the end of the file at top level, else to
            // its closing brace.
            let attr = line.trim();
            let test_attr = attr == "#[cfg(test)]" || attr.starts_with("#[cfg(all(test");
            if test_attr && !line.starts_with(' ') {
                break;
            }
            if test_attr
                && lines[i + 1..].iter().take(3).any(|l| {
                    l.trim_start().starts_with("mod ") || l.trim_start().starts_with("pub mod ")
                })
            {
                let indent = &line[..line.len() - line.trim_start().len()];
                let close = format!("{indent}}}");
                skip_until = lines[i..]
                    .iter()
                    .position(|l| *l == close)
                    .map_or(lines.len(), |p| i + p + 1);
                continue;
            }
            let code = line.trim_start();
            if code.starts_with("//") || !line.contains("Command::new(") {
                continue;
            }
            let allowed = |l: &str| l.contains("// hidden:") || l.contains("// visible-console:");
            if allowed(line) || (i > 0 && allowed(lines[i - 1])) {
                continue;
            }
            let rel = file.strip_prefix(root).unwrap_or(file);
            offenders.push(format!("{}:{}: {}", rel.display(), i + 1, line.trim()));
        }
    }
    assert!(
        offenders.is_empty(),
        "bare `Command::new` opens a console window on Windows. Use \
         `harness_core::process::hidden_command` / `hidden_tokio_command` (or `quiet_command`), \
         or mark a deliberate exception with `// visible-console: <why>` / `// hidden: <why>`. \
         Fix:\n{}",
        offenders.join("\n")
    );
}
