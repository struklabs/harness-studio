//! Text of an attached file: PDF (per page), DOCX, Markdown, source code and plain text.
//!
//! Everything is pure Rust and local:
//! * **PDF** — `pdf-extract` (MIT, on `lopdf`), page by page. It still panics on some
//!   malformed files and the release profile aborts on panic, so release builds extract in a
//!   child process: the app's own executable started with [`CHILD_FLAG`] (see
//!   [`child_main`], called first thing in `main` of the GUI and the CLI). A crash or a hang
//!   (killed after [`PDF_TIMEOUT`]) only fails that document. Builds with `panic = "unwind"`
//!   (debug, tests) extract in-process behind `catch_unwind`.
//!   Scanned PDFs without a text layer give `rag.pdf_no_text` (there is no OCR).
//! * **DOCX** — `word/document.xml` from the zip; paragraphs, tabs and breaks, headings
//!   (`HeadingN` styles) become Markdown `#` lines so the chunker keeps them.
//! * **Text** — UTF-8 (BOM stripped), UTF-16 with BOM, otherwise Windows-1252. Files with NUL
//!   bytes are treated as binary and rejected.

use crate::error_key::err;
use serde::{Deserialize, Serialize};
use std::io::Read;
use std::path::Path;
use std::time::Duration;

/// Largest file accepted (bytes).
pub const MAX_FILE_BYTES: u64 = 64 * 1024 * 1024;
/// Largest extracted text kept (characters); longer documents are rejected, not truncated.
pub const MAX_TEXT_CHARS: usize = 4_000_000;
/// Argument that turns the app's executable into the PDF extraction child.
pub const CHILD_FLAG: &str = "--harness-extract-pdf";
/// A PDF child that runs longer than this is killed.
pub const PDF_TIMEOUT: Duration = Duration::from_secs(180);

/// What kind of file a document came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DocKind {
    Pdf,
    Docx,
    Markdown,
    Code,
    Text,
}

impl DocKind {
    pub fn as_str(self) -> &'static str {
        match self {
            DocKind::Pdf => "pdf",
            DocKind::Docx => "docx",
            DocKind::Markdown => "markdown",
            DocKind::Code => "code",
            DocKind::Text => "text",
        }
    }
}

/// Text of one page (`number` is 1-based; `None` for formats without pages).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PageText {
    pub number: Option<u32>,
    pub text: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Extracted {
    pub kind: DocKind,
    pub pages: Vec<PageText>,
}

impl Extracted {
    pub fn chars(&self) -> usize {
        self.pages.iter().map(|p| p.text.chars().count()).sum()
    }

    /// Page count for paged formats.
    pub fn page_count(&self) -> Option<u32> {
        self.pages.iter().filter_map(|p| p.number).max()
    }

    /// The whole text; pages are separated by a `[Page N]` line (used by "full text" mode).
    pub fn full_text(&self) -> String {
        let mut out = String::new();
        for p in &self.pages {
            if let Some(n) = p.number {
                if !out.is_empty() {
                    out.push_str("\n\n");
                }
                out.push_str(&format!("[Page {n}]\n"));
            }
            out.push_str(p.text.trim_end());
        }
        out
    }
}

const MARKDOWN: &[&str] = &["md", "markdown", "mdx", "rst", "adoc"];
const CODE: &[&str] = &[
    "rs",
    "py",
    "js",
    "mjs",
    "cjs",
    "ts",
    "tsx",
    "jsx",
    "svelte",
    "vue",
    "go",
    "java",
    "kt",
    "kts",
    "c",
    "h",
    "cc",
    "cpp",
    "cxx",
    "hpp",
    "cs",
    "rb",
    "php",
    "swift",
    "m",
    "mm",
    "scala",
    "dart",
    "lua",
    "r",
    "jl",
    "pl",
    "sh",
    "bash",
    "zsh",
    "fish",
    "ps1",
    "psm1",
    "bat",
    "cmd",
    "sql",
    "css",
    "scss",
    "sass",
    "less",
    "html",
    "htm",
    "xml",
    "json",
    "jsonc",
    "jsonl",
    "yaml",
    "yml",
    "toml",
    "ini",
    "cfg",
    "conf",
    "gradle",
    "cmake",
    "zig",
    "nim",
    "ex",
    "exs",
    "erl",
    "hs",
    "ml",
    "fs",
    "proto",
    "graphql",
    "tf",
    "dockerfile",
    "makefile",
];
const TEXT: &[&str] = &[
    "txt", "text", "log", "csv", "tsv", "srt", "vtt", "tex", "bib", "org",
];

/// Extensions offered in the file picker (other text files are accepted too).
pub fn supported_extensions() -> Vec<&'static str> {
    let mut v = vec!["pdf", "docx"];
    v.extend(MARKDOWN);
    v.extend(TEXT);
    v.extend(
        CODE.iter()
            .filter(|e| !matches!(**e, "dockerfile" | "makefile")),
    );
    v
}

/// Kind from the file name; `None` = unknown extension (accepted when the content is text).
pub fn kind_of(path: &Path) -> Option<DocKind> {
    let name = path.file_name()?.to_string_lossy().to_lowercase();
    if matches!(name.as_str(), "dockerfile" | "makefile" | "cmakelists.txt") {
        return Some(DocKind::Code);
    }
    let ext = path.extension()?.to_string_lossy().to_lowercase();
    match ext.as_str() {
        "pdf" => Some(DocKind::Pdf),
        "docx" => Some(DocKind::Docx),
        e if MARKDOWN.contains(&e) => Some(DocKind::Markdown),
        e if CODE.contains(&e) => Some(DocKind::Code),
        e if TEXT.contains(&e) => Some(DocKind::Text),
        _ => None,
    }
}

/// Formats known to be binary (rejected without reading).
fn known_binary(path: &Path) -> bool {
    const BIN: &[&str] = &[
        "doc", "xls", "xlsx", "ppt", "pptx", "odt", "rtf", "epub", "zip", "7z", "rar", "gz", "exe",
        "dll", "png", "jpg", "jpeg", "gif", "webp", "bmp", "mp3", "mp4", "wav", "gguf", "bin",
    ];
    path.extension()
        .map(|e| BIN.contains(&e.to_string_lossy().to_lowercase().as_str()))
        .unwrap_or(false)
}

/// Reads and extracts a file (blocking; PDFs may start a child process).
pub fn extract_file(path: &Path) -> Result<Extracted, String> {
    let meta = std::fs::metadata(path).map_err(|e| {
        String::from(
            err("fs.read_failed")
                .with("path", path.display())
                .with("error", e),
        )
    })?;
    if !meta.is_file() {
        return Err(err("rag.not_a_file").with("path", path.display()).into());
    }
    if meta.len() > MAX_FILE_BYTES {
        return Err(err("rag.file_too_large")
            .with("name", file_name(path))
            .num("max_mib", MAX_FILE_BYTES / (1024 * 1024))
            .into());
    }
    if known_binary(path) {
        return Err(err("rag.unsupported_file")
            .with("name", file_name(path))
            .into());
    }
    let bytes = std::fs::read(path).map_err(|e| {
        String::from(
            err("fs.read_failed")
                .with("path", path.display())
                .with("error", e),
        )
    })?;
    let kind = kind_of(path);
    let out = match kind {
        Some(DocKind::Pdf) => Extracted {
            kind: DocKind::Pdf,
            pages: pdf_pages(path, &bytes)?,
        },
        Some(DocKind::Docx) => Extracted {
            kind: DocKind::Docx,
            pages: vec![PageText {
                number: None,
                text: docx_text(&bytes).map_err(|e| {
                    String::from(
                        err("rag.docx_failed")
                            .with("name", file_name(path))
                            .with("error", e),
                    )
                })?,
            }],
        },
        other => {
            let text = decode_text(&bytes).ok_or_else(|| {
                String::from(err("rag.unsupported_file").with("name", file_name(path)))
            })?;
            Extracted {
                kind: other.unwrap_or(DocKind::Text),
                pages: vec![PageText {
                    number: None,
                    text: normalize(&text),
                }],
            }
        }
    };
    let chars = out.chars();
    if chars == 0 || out.pages.iter().all(|p| p.text.trim().is_empty()) {
        return Err(if out.kind == DocKind::Pdf {
            err("rag.pdf_no_text").with("name", file_name(path)).into()
        } else {
            err("rag.empty_document")
                .with("name", file_name(path))
                .into()
        });
    }
    if chars > MAX_TEXT_CHARS {
        return Err(err("rag.text_too_long")
            .with("name", file_name(path))
            .num("chars", chars as u64)
            .num("max", MAX_TEXT_CHARS as u64)
            .into());
    }
    Ok(out)
}

pub fn file_name(path: &Path) -> String {
    path.file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| path.display().to_string())
}

// ---- text ----

/// Decodes text bytes; `None` for binary content.
pub fn decode_text(bytes: &[u8]) -> Option<String> {
    if let Some((enc, bom)) = encoding_rs::Encoding::for_bom(bytes) {
        let (text, _) = enc.decode_without_bom_handling(&bytes[bom..]);
        return Some(text.into_owned());
    }
    let head = &bytes[..bytes.len().min(8192)];
    if head.contains(&0) {
        return None;
    }
    match std::str::from_utf8(bytes) {
        Ok(s) => Some(s.to_string()),
        Err(_) => {
            let (text, _, _) = encoding_rs::WINDOWS_1252.decode(bytes);
            Some(text.into_owned())
        }
    }
}

/// Line endings to `\n`, no trailing spaces, at most two consecutive blank lines, no
/// control characters other than tab/newline.
pub fn normalize(text: &str) -> String {
    let text = text.replace("\r\n", "\n").replace('\r', "\n");
    let mut out = String::with_capacity(text.len());
    let mut blank = 0;
    for line in text.lines() {
        let line: String = line
            .chars()
            .filter(|c| !c.is_control() || *c == '\t')
            .collect();
        let line = line.trim_end();
        if line.is_empty() {
            blank += 1;
            if blank > 1 {
                continue;
            }
        } else {
            blank = 0;
        }
        out.push_str(line);
        out.push('\n');
    }
    out.trim_matches('\n').to_string()
}

// ---- DOCX ----

/// Paragraph text of a DOCX (`word/document.xml`).
pub fn docx_text(bytes: &[u8]) -> Result<String, String> {
    let mut zip = zip::ZipArchive::new(std::io::Cursor::new(bytes)).map_err(|e| e.to_string())?;
    let mut xml = String::new();
    zip.by_name("word/document.xml")
        .map_err(|e| e.to_string())?
        .read_to_string(&mut xml)
        .map_err(|e| e.to_string())?;
    Ok(normalize(&docx_xml_text(&xml)))
}

/// Walks WordprocessingML: `w:t` text, `w:tab`, `w:br`/`w:cr`, paragraph ends, and
/// `w:pStyle` `HeadingN` / `Title` as Markdown headings.
fn docx_xml_text(xml: &str) -> String {
    let mut out = String::new();
    let mut para = String::new();
    let mut heading: Option<usize> = None;
    let mut in_text = false;
    let mut rest = xml;
    while let Some(lt) = rest.find('<') {
        if in_text {
            para.push_str(&decode_entities(&rest[..lt]));
        }
        let Some(gt) = rest[lt..].find('>') else {
            break;
        };
        let tag = &rest[lt + 1..lt + gt];
        rest = &rest[lt + gt + 1..];
        let closing = tag.starts_with('/');
        let body = tag.trim_start_matches('/').trim_end_matches('/');
        let name = body.split_whitespace().next().unwrap_or("");
        match name {
            "w:t" => in_text = !closing && !tag.ends_with('/'),
            "w:tab" if !closing => para.push('\t'),
            "w:br" | "w:cr" if !closing => para.push('\n'),
            "w:pStyle" => {
                let val = attr(body, "w:val").unwrap_or_default().to_lowercase();
                heading = if val == "title" {
                    Some(1)
                } else {
                    val.strip_prefix("heading")
                        .and_then(|n| n.parse::<usize>().ok())
                        .map(|n| n.clamp(1, 6))
                };
            }
            "w:p" if closing || tag.ends_with('/') => {
                let text = para.trim();
                if !text.is_empty() {
                    if let Some(level) = heading {
                        out.push_str(&"#".repeat(level));
                        out.push(' ');
                    }
                    out.push_str(text);
                    out.push_str("\n\n");
                }
                para.clear();
                heading = None;
                in_text = false;
            }
            _ => {}
        }
    }
    out
}

fn attr(tag: &str, name: &str) -> Option<String> {
    let pos = tag.find(&format!("{name}=\""))?;
    let rest = &tag[pos + name.len() + 2..];
    Some(rest[..rest.find('"')?].to_string())
}

fn decode_entities(s: &str) -> String {
    if !s.contains('&') {
        return s.to_string();
    }
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(amp) = rest.find('&') {
        out.push_str(&rest[..amp]);
        let after = &rest[amp + 1..];
        let Some(semi) = after.find(';').filter(|&i| i <= 10) else {
            out.push('&');
            rest = after;
            continue;
        };
        let ent = &after[..semi];
        let ch = match ent {
            "amp" => Some('&'),
            "lt" => Some('<'),
            "gt" => Some('>'),
            "quot" => Some('"'),
            "apos" => Some('\''),
            e if e.starts_with("#x") || e.starts_with("#X") => u32::from_str_radix(&e[2..], 16)
                .ok()
                .and_then(char::from_u32),
            e if e.starts_with('#') => e[1..].parse::<u32>().ok().and_then(char::from_u32),
            _ => None,
        };
        match ch {
            Some(c) => {
                out.push(c);
                rest = &after[semi + 1..];
            }
            None => {
                out.push('&');
                rest = after;
            }
        }
    }
    out.push_str(rest);
    out
}

// ---- PDF ----

/// Pages of a PDF (in-process with `panic = "unwind"`, else in a child process).
fn pdf_pages(path: &Path, bytes: &[u8]) -> Result<Vec<PageText>, String> {
    #[cfg(panic = "unwind")]
    {
        let _ = path;
        pdf_pages_in_process(bytes).map_err(|e| pdf_error(path, &e))
    }
    #[cfg(not(panic = "unwind"))]
    {
        let _ = bytes;
        pdf_pages_child(path)
    }
}

fn pdf_error(path: &Path, detail: &str) -> String {
    err("rag.pdf_failed")
        .with("name", file_name(path))
        .with("error", detail)
        .into()
}

/// In-process extraction; a panic inside `pdf-extract` becomes an error (needs unwinding).
pub fn pdf_pages_in_process(bytes: &[u8]) -> Result<Vec<PageText>, String> {
    let owned = bytes.to_vec();
    let result =
        std::panic::catch_unwind(move || pdf_extract::extract_text_from_mem_by_pages(&owned));
    match result {
        Ok(Ok(pages)) => Ok(pages
            .into_iter()
            .enumerate()
            .map(|(i, text)| PageText {
                number: Some(i as u32 + 1),
                text: normalize(&text),
            })
            .collect()),
        Ok(Err(e)) => Err(e.to_string()),
        Err(panic) => Err(panic
            .downcast_ref::<String>()
            .cloned()
            .or_else(|| panic.downcast_ref::<&str>().map(|s| s.to_string()))
            .unwrap_or_else(|| "pdf parser panicked".into())),
    }
}

#[derive(Serialize, Deserialize)]
struct ChildOutput {
    #[serde(default)]
    pages: Vec<String>,
    #[serde(default)]
    error: Option<String>,
}

/// Extraction in a child process of the running executable (see [`child_main`]).
pub fn pdf_pages_child(path: &Path) -> Result<Vec<PageText>, String> {
    let exe = std::env::current_exe().map_err(|e| pdf_error(path, &e.to_string()))?;
    pdf_pages_with(&exe, path)
}

/// Runs `exe --harness-extract-pdf <path>` and reads its JSON answer.
pub fn pdf_pages_with(exe: &Path, path: &Path) -> Result<Vec<PageText>, String> {
    use std::process::{Command, Stdio};
    let mut cmd = Command::new(exe);
    cmd.arg(CHILD_FLAG)
        .arg(path)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
    }
    let mut child = cmd.spawn().map_err(|e| pdf_error(path, &e.to_string()))?;
    let mut stdout = child.stdout.take();
    let reader = crate::resource_io::spawn_thread("harness-pdf-output", move || {
        let mut out = Vec::new();
        if let Some(s) = stdout.as_mut() {
            let _ = s.read_to_end(&mut out);
        }
        out
    });
    let reader = match reader {
        Ok(reader) => reader,
        Err(e) => {
            let _ = child.kill();
            let _ = child.wait();
            return Err(pdf_error(path, &e.to_string()));
        }
    };
    let started = std::time::Instant::now();
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) if started.elapsed() > PDF_TIMEOUT => {
                let _ = child.kill();
                let _ = child.wait();
                let _ = reader.join();
                return Err(err("rag.pdf_timeout")
                    .with("name", file_name(path))
                    .num("seconds", PDF_TIMEOUT.as_secs())
                    .into());
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(50)),
            Err(e) => return Err(pdf_error(path, &e.to_string())),
        }
    };
    let out = reader.join().unwrap_or_default();
    let parsed: Option<ChildOutput> = serde_json::from_slice(&out).ok();
    match parsed {
        Some(ChildOutput { error: Some(e), .. }) => Err(pdf_error(path, &e)),
        Some(ChildOutput { pages, .. }) if status.success() => Ok(pages
            .into_iter()
            .enumerate()
            .map(|(i, text)| PageText {
                number: Some(i as u32 + 1),
                text,
            })
            .collect()),
        _ => Err(pdf_error(path, &format!("extractor exited with {status}"))),
    }
}

/// Entry of the extraction child: when the process was started with [`CHILD_FLAG`], extracts
/// the PDF, prints JSON (`{"pages": [...]}` or `{"error": "..."}`) and returns the exit code.
/// Returns `None` for a normal start. Call first thing in `main`.
pub fn child_main() -> Option<i32> {
    let mut args = std::env::args_os().skip(1);
    if args.next()? != CHILD_FLAG {
        return None;
    }
    let path = args.next()?;
    let out = match std::fs::read(&path) {
        Err(e) => ChildOutput {
            pages: Vec::new(),
            error: Some(e.to_string()),
        },
        Ok(bytes) => match pdf_extract::extract_text_from_mem_by_pages(&bytes) {
            Ok(pages) => ChildOutput {
                pages: pages.iter().map(|p| normalize(p)).collect(),
                error: None,
            },
            Err(e) => ChildOutput {
                pages: Vec::new(),
                error: Some(e.to_string()),
            },
        },
    };
    let ok = out.error.is_none();
    let json = serde_json::to_string(&out).unwrap_or_default();
    use std::io::Write;
    let mut stdout = std::io::stdout().lock();
    let _ = stdout.write_all(json.as_bytes());
    let _ = stdout.flush();
    Some(if ok { 0 } else { 2 })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    /// Real files printed by Microsoft Edge (`--print-to-pdf`) and saved by Microsoft Word
    /// (`SaveAs2`, .docx) from one HTML page: the `state.db` schema section of
    /// docs/en/ARCHITECTURE.md, v1 tables on page 1 and the v2–v4 migrations on page 2.
    pub(crate) fn fixture(name: &str) -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/data/rag")
            .join(name)
    }

    fn squash(s: &str) -> String {
        s.split_whitespace().collect::<Vec<_>>().join(" ")
    }

    #[test]
    fn pdf_pages_keep_page_numbers() {
        let doc = extract_file(&fixture("state-db-schema.pdf")).unwrap();
        assert_eq!(doc.kind, DocKind::Pdf);
        assert_eq!(doc.page_count(), Some(2));
        let p1 = squash(&doc.pages[0].text);
        let p2 = squash(&doc.pages[1].text);
        assert!(p1.contains("Harness Studio state.db şeması"), "{p1}");
        assert!(
            p1.contains("folders(id, name, parent_id, sort, created_at)"),
            "{p1}"
        );
        assert!(!p1.contains("vram_measurements"));
        assert!(
            p2.contains("vram_measurements(model_key, settings_key"),
            "{p2}"
        );
        assert!(p2.contains("birincil anahtar"), "{p2}");
        let full = doc.full_text();
        assert!(full.starts_with("[Page 1]\n"));
        assert!(full.contains("\n\n[Page 2]\n"));
    }

    #[test]
    fn docx_paragraphs_and_headings() {
        let doc = extract_file(&fixture("state-db-schema.docx")).unwrap();
        assert_eq!(doc.kind, DocKind::Docx);
        let text = &doc.pages[0].text;
        assert!(
            text.starts_with("# Harness Studio state.db şeması\n\n## v1 tablolar"),
            "{text}"
        );
        assert!(
            text.contains("\n\nfolders(id, name, parent_id, sort, created_at). Klasör"),
            "{text}"
        );
        assert!(text.contains("vram_measurements(model_key, settings_key, model_size"));
        // No XML left over.
        assert!(!text.contains("w:t") && !text.contains('<'));
    }

    #[test]
    fn docx_xml_entities_tabs_and_breaks() {
        let xml = r#"<w:body><w:p><w:pPr><w:pStyle w:val="Heading2"/></w:pPr><w:r><w:t>A &amp; B</w:t></w:r></w:p><w:p><w:r><w:t xml:space="preserve">x </w:t><w:tab/><w:t>&lt;y&gt; &#252;&#x131;</w:t><w:br/><w:t>z</w:t></w:r></w:p><w:p/></w:body>"#;
        assert_eq!(docx_xml_text(xml), "## A & B\n\nx \t<y> üı\nz\n\n");
    }

    #[test]
    fn text_encodings_and_binary() {
        assert_eq!(decode_text("şeker".as_bytes()).unwrap(), "şeker");
        assert_eq!(decode_text(b"\xEF\xBB\xBFbom").unwrap(), "bom");
        let utf16: Vec<u8> = [0xFF, 0xFE]
            .into_iter()
            .chain("ağ".encode_utf16().flat_map(|u| u.to_le_bytes()))
            .collect();
        assert_eq!(decode_text(&utf16).unwrap(), "ağ");
        assert_eq!(decode_text(b"caf\xE9").unwrap(), "café");
        assert_eq!(decode_text(b"\x00\x01binary"), None);
    }

    #[test]
    fn normalize_collapses_blank_lines() {
        assert_eq!(
            normalize("a  \r\n\r\n\r\n\r\nb\u{0007}\tc\n\n"),
            "a\n\nb\tc"
        );
    }

    #[test]
    fn kinds_by_extension() {
        assert_eq!(kind_of(Path::new("a/B.PDF")), Some(DocKind::Pdf));
        assert_eq!(kind_of(Path::new("x.md")), Some(DocKind::Markdown));
        assert_eq!(kind_of(Path::new("main.rs")), Some(DocKind::Code));
        assert_eq!(kind_of(Path::new("Dockerfile")), Some(DocKind::Code));
        assert_eq!(kind_of(Path::new("notes.txt")), Some(DocKind::Text));
        assert_eq!(kind_of(Path::new("README")), None);
    }

    #[test]
    fn rejects_binary_and_empty_files() {
        let dir = tempfile::tempdir().unwrap();
        let img = dir.path().join("a.png");
        std::fs::write(&img, b"\x89PNG").unwrap();
        let e = extract_file(&img).unwrap_err();
        assert!(e.starts_with("rag.unsupported_file"), "{e}");
        let empty = dir.path().join("e.txt");
        std::fs::write(&empty, " \n\n").unwrap();
        let e = extract_file(&empty).unwrap_err();
        assert!(e.starts_with("rag.empty_document"), "{e}");
        // A real source file of this crate reads as code.
        let own = Path::new(env!("CARGO_MANIFEST_DIR")).join("src/rag/extract.rs");
        let doc = extract_file(&own).unwrap();
        assert_eq!(doc.kind, DocKind::Code);
        assert!(doc.pages[0].text.contains("pub fn child_main()"));
    }

    #[test]
    fn pdf_child_reports_a_failing_extractor() {
        // A child that is not the extractor (exits without JSON) fails that document only.
        let exe = PathBuf::from(if cfg!(windows) { "cmd.exe" } else { "true" });
        let e = pdf_pages_with(&exe, &fixture("state-db-schema.pdf")).unwrap_err();
        assert!(e.starts_with("rag.pdf_failed"), "{e}");
    }
}
