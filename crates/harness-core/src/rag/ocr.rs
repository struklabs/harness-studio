//! Text recognition (OCR) for scanned PDFs, shared by the gateway's `/v1/ocr` and
//! `/v1/document/qa` routes and by knowledge base indexing.
//!
//! * **Page selection** ([`plan_pages`]): a page whose text layer has text is read as text,
//!   a page without one (a scan) is recognized. A mixed PDF uses both.
//! * **Rasterizing** ([`rasterize`]): pdfium through `pypdfium2` in the managed Python
//!   environment of the `python-hf` engine (`python/pdf_raster.py`). The wheel bundles pdfium
//!   and needs nothing on the system; PyMuPDF was not taken because it is AGPL-3.0 and this
//!   project is MIT. Without the environment the error is `ocr.rasterizer_unavailable`
//!   (install the Python engine); there is no silent fallback.
//! * **Limits** ([`Limits`]): pages per request, render DPI and a pixel cap per page, with
//!   environment overrides (`HARNESS_OCR_MAX_PAGES`, `HARNESS_OCR_DPI`,
//!   `HARNESS_KB_OCR_MAX_PAGES`).
//! * **Indexing** ([`extract_for_index`]): the pages of a knowledge base document that have
//!   no text layer go through the OCR model of the base, a few at a time.

use super::extract::{self, Extracted, PageText};
use super::{blocking_or_cancel, Endpoint};
use crate::error_key::err;
use std::future::Future;
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

/// Pages recognized per request of the gateway (and the longest PDF it takes).
pub const DEFAULT_MAX_PAGES: usize = 16;
/// Pages recognized per knowledge base document.
pub const KB_DEFAULT_MAX_PAGES: usize = 200;
/// Render resolution (dots per inch) unless overridden.
pub const DEFAULT_DPI: u32 = 150;
pub const MIN_DPI: u32 = 72;
pub const MAX_DPI: u32 = 300;
/// Longest side of a rendered page in pixels (large pages are scaled down to it).
pub const MAX_EDGE_PX: u32 = 3000;
/// Largest rendered page (PNG bytes) taken from the rasterizer.
pub const MAX_PAGE_PNG_BYTES: u64 = 12 * 1024 * 1024;
/// Pages rendered per rasterizer run while indexing (bounds memory and the disk used).
const BATCH: usize = 8;
/// A rasterizer run that takes longer than this is killed.
const RASTER_TIMEOUT: Duration = Duration::from_secs(180);
/// A page needs this many letters or digits in its text layer to count as text.
const MIN_TEXT_ALNUM: usize = 3;

const SCRIPT: &str = include_str!("../../python/pdf_raster.py");

/// Limits of one OCR run.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Limits {
    pub max_pages: usize,
    pub dpi: u32,
}

impl Limits {
    /// Gateway requests: [`DEFAULT_MAX_PAGES`] unless `HARNESS_OCR_MAX_PAGES` says otherwise.
    pub fn from_env() -> Self {
        Self::parse(
            std::env::var("HARNESS_OCR_MAX_PAGES").ok().as_deref(),
            std::env::var("HARNESS_OCR_DPI").ok().as_deref(),
            DEFAULT_MAX_PAGES,
        )
    }

    /// Knowledge base indexing: [`KB_DEFAULT_MAX_PAGES`] unless `HARNESS_KB_OCR_MAX_PAGES`.
    pub fn kb_from_env() -> Self {
        Self::parse(
            std::env::var("HARNESS_KB_OCR_MAX_PAGES").ok().as_deref(),
            std::env::var("HARNESS_OCR_DPI").ok().as_deref(),
            KB_DEFAULT_MAX_PAGES,
        )
    }

    /// Reads the two settings; a value that is not a number in range falls back to the default.
    pub fn parse(max_pages: Option<&str>, dpi: Option<&str>, default_pages: usize) -> Self {
        let number = |v: Option<&str>| v.and_then(|s| s.trim().parse::<usize>().ok());
        Self {
            max_pages: number(max_pages)
                .filter(|n| (1..=1000).contains(n))
                .unwrap_or(default_pages),
            dpi: number(dpi)
                .filter(|n| (MIN_DPI as usize..=MAX_DPI as usize).contains(n))
                .map_or(DEFAULT_DPI, |n| n as u32),
        }
    }
}

/// Where the text of a page comes from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PageSource {
    TextLayer,
    Ocr,
}

/// Whether a text layer holds text (a few letters or digits; stray marks do not count).
pub fn has_text_layer(text: &str) -> bool {
    text.chars().filter(|c| c.is_alphanumeric()).count() >= MIN_TEXT_ALNUM
}

/// Which pages are read as text and which are recognized (index = page number - 1).
pub fn plan_pages(pages: &[PageText]) -> Vec<PageSource> {
    pages
        .iter()
        .map(|p| {
            if has_text_layer(&p.text) {
                PageSource::TextLayer
            } else {
                PageSource::Ocr
            }
        })
        .collect()
}

/// The 1-based numbers of the pages to recognize.
pub fn ocr_page_numbers(plan: &[PageSource]) -> Vec<u32> {
    plan.iter()
        .enumerate()
        .filter(|(_, s)| **s == PageSource::Ocr)
        .map(|(i, _)| i as u32 + 1)
        .collect()
}

/// One rendered page.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RasterPage {
    pub number: u32,
    pub png: Vec<u8>,
}

/// Renders `pages` (1-based) of the PDF at `pdf` with the Python engine's environment.
/// Blocking. `ocr.rasterizer_unavailable` when that environment is not installed.
pub fn rasterize(pdf: &Path, pages: &[u32], limits: &Limits) -> Result<Vec<RasterPage>, String> {
    rasterize_in(&crate::pyenv::envs::root(), pdf, pages, limits)
}

/// [`rasterize`] with the environments under `root`.
pub fn rasterize_in(
    root: &Path,
    pdf: &Path,
    pages: &[u32],
    limits: &Limits,
) -> Result<Vec<RasterPage>, String> {
    if pages.is_empty() {
        return Ok(Vec::new());
    }
    let python = crate::engine::python_hf::binary_in(root)
        .map_err(|_| String::from(err("ocr.rasterizer_unavailable")))?;
    let script = ensure_script(root)?;
    let out = tempfile::Builder::new()
        .prefix("harness-ocr-")
        .tempdir()
        .map_err(|e| String::from(err("ocr.rasterize_failed").with("error", e)))?;
    let list = pages
        .iter()
        .map(u32::to_string)
        .collect::<Vec<_>>()
        .join(",");
    let stdout = run_script(&python, &script, pdf, out.path(), limits, &list)?;
    parse_output(&stdout, out.path())
}

fn ensure_script(root: &Path) -> Result<PathBuf, String> {
    let dir = crate::pyenv::envs::env_dir_in(root, crate::engine::python_hf::env_in(root).name);
    let path = dir.join("pdf_raster.py");
    if std::fs::read_to_string(&path).ok().as_deref() == Some(SCRIPT) {
        return Ok(path);
    }
    std::fs::create_dir_all(&dir)
        .and_then(|_| std::fs::write(&path, SCRIPT))
        .map_err(|e| {
            String::from(
                err("fs.write_failed")
                    .with("path", path.display())
                    .with("error", e),
            )
        })?;
    Ok(path)
}

fn run_script(
    python: &Path,
    script: &Path,
    pdf: &Path,
    out_dir: &Path,
    limits: &Limits,
    pages: &str,
) -> Result<String, String> {
    use std::io::Read;
    use std::process::Stdio;
    let failed = |e: String| String::from(err("ocr.rasterize_failed").with("error", e));
    let mut cmd = crate::process::hidden_command(python);
    cmd.arg(script)
        .arg(pdf)
        .arg(out_dir)
        .arg(limits.dpi.to_string())
        .arg(MAX_EDGE_PX.to_string())
        .arg(pages)
        .env("PYTHONNOUSERSITE", "1")
        .env("PYTHONIOENCODING", "utf-8")
        .env("MPLBACKEND", "Agg")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
    }
    let mut child = cmd.spawn().map_err(|e| failed(e.to_string()))?;
    let mut stdout = child.stdout.take();
    let mut stderr = child.stderr.take();
    let read_out = crate::resource_io::spawn_thread("harness-ocr-raster-out", move || {
        let mut buf = Vec::new();
        if let Some(s) = stdout.as_mut() {
            let _ = s.read_to_end(&mut buf);
        }
        buf
    })
    .map_err(|e| failed(e.to_string()))?;
    let read_err = crate::resource_io::spawn_thread("harness-ocr-raster-err", move || {
        let mut buf = Vec::new();
        if let Some(s) = stderr.as_mut() {
            let _ = s.take(8192).read_to_end(&mut buf);
        }
        buf
    })
    .map_err(|e| failed(e.to_string()))?;
    let started = std::time::Instant::now();
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) if started.elapsed() > RASTER_TIMEOUT => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(failed(format!(
                    "timed out after {} seconds",
                    RASTER_TIMEOUT.as_secs()
                )));
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(50)),
            Err(e) => return Err(failed(e.to_string())),
        }
    };
    let out = String::from_utf8_lossy(&read_out.join().unwrap_or_default()).into_owned();
    let errors = String::from_utf8_lossy(&read_err.join().unwrap_or_default()).into_owned();
    if !status.success() {
        let tail = errors.lines().last().unwrap_or("").to_string();
        return Err(failed(format!("rasterizer exited with {status}: {tail}")));
    }
    Ok(out)
}

/// Reads the rasterizer's JSON answer and the page files it names (inside `dir`).
pub fn parse_output(stdout: &str, dir: &Path) -> Result<Vec<RasterPage>, String> {
    let failed = |e: String| String::from(err("ocr.rasterize_failed").with("error", e));
    let line = stdout
        .lines()
        .rev()
        .find(|l| l.starts_with('{'))
        .unwrap_or("");
    let v: serde_json::Value =
        serde_json::from_str(line).map_err(|_| failed("the rasterizer gave no answer".into()))?;
    if v["ok"] != true {
        let code = v["error"].as_str().unwrap_or("unknown");
        if code == "missing_module" {
            return Err(err("ocr.rasterizer_unavailable").into());
        }
        let detail = v["detail"].as_str().unwrap_or_default();
        return Err(failed(format!("{code} {detail}").trim().to_string()));
    }
    let mut out = Vec::new();
    for p in v["pages"].as_array().into_iter().flatten() {
        let number = p["number"].as_u64().unwrap_or(0) as u32;
        let file = PathBuf::from(p["file"].as_str().unwrap_or_default());
        // Only files the rasterizer wrote into its own folder are read.
        if number == 0 || file.parent() != Some(dir) {
            return Err(failed("the rasterizer named an unexpected file".into()));
        }
        let size = std::fs::metadata(&file)
            .map_err(|e| failed(e.to_string()))?
            .len();
        if size > MAX_PAGE_PNG_BYTES {
            return Err(err("ocr.page_too_large")
                .num("page", number)
                .num("max_mib", MAX_PAGE_PNG_BYTES / (1024 * 1024))
                .into());
        }
        let png = std::fs::read(&file).map_err(|e| failed(e.to_string()))?;
        out.push(RasterPage { number, png });
    }
    Ok(out)
}

// ---- recognizing --------------------------------------------------------------------------

/// Replaces the Python rasterizer (tests).
pub type Rasterizer =
    Arc<dyn Fn(&Path, &[u32], &Limits) -> Result<Vec<RasterPage>, String> + Send + Sync>;

/// Recognizes an image file given its bytes and MIME type.
type ImageRunner = Arc<dyn Fn(Vec<u8>, &'static str) -> OcrFuture + Send + Sync>;

pub type OcrFuture = Pin<Box<dyn Future<Output = Result<String, String>> + Send>>;

/// The OCR model of a knowledge base: recognizes one page image (PNG) at a time.
#[derive(Clone)]
pub struct OcrModel {
    pub alias: String,
    run: Arc<dyn Fn(Vec<u8>) -> OcrFuture + Send + Sync>,
    /// Recognizes an image file of any type (bytes, MIME type); falls back to `run`.
    run_image: Option<ImageRunner>,
    render: Option<Rasterizer>,
}

impl std::fmt::Debug for OcrModel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("OcrModel")
            .field("alias", &self.alias)
            .finish_non_exhaustive()
    }
}

impl OcrModel {
    pub fn new(
        alias: impl Into<String>,
        run: impl Fn(Vec<u8>) -> OcrFuture + Send + Sync + 'static,
    ) -> Self {
        Self {
            alias: alias.into(),
            run: Arc::new(run),
            run_image: None,
            render: None,
        }
    }

    /// Renders pages with `render` instead of the Python engine (tests).
    #[doc(hidden)]
    pub fn with_rasterizer(mut self, render: Rasterizer) -> Self {
        self.render = Some(render);
        self
    }

    /// Recognizes pages with the gateway's `/v1/ocr` route (the route the Read Document panel
    /// and the API use) on this machine, by the model's alias.
    pub fn gateway(endpoint: Endpoint, alias: String) -> Self {
        let model = alias.clone();
        let (ep2, model2) = (endpoint.clone(), model.clone());
        let mut this = Self::new(alias, move |png| {
            let (endpoint, model) = (endpoint.clone(), model.clone());
            Box::pin(async move { super::client::ocr(&endpoint, &model, &png).await })
        });
        this.run_image = Some(Arc::new(move |bytes, mime| {
            let (endpoint, model) = (ep2.clone(), model2.clone());
            Box::pin(async move { super::client::ocr_image(&endpoint, &model, &bytes, mime).await })
        }));
        this
    }

    /// Recognizes an image file (`mime` is its `image/...` type).
    pub async fn recognize_image(
        &self,
        bytes: Vec<u8>,
        mime: &'static str,
    ) -> Result<String, String> {
        match &self.run_image {
            Some(run) => run(bytes, mime).await,
            None => (self.run)(bytes).await,
        }
    }

    pub async fn recognize(&self, png: Vec<u8>) -> Result<String, String> {
        (self.run)(png).await
    }
}

/// Recognizes the pages of `doc` (read from the PDF at `path`) that have no text layer and
/// puts their text in place. Returns the numbers of the pages recognized. The rasterizing runs
/// a few pages at a time; `cancel` is looked at between pages.
pub async fn recognize_blank_pages(
    path: &Path,
    doc: &mut Extracted,
    model: &OcrModel,
    limits: &Limits,
    cancel: &AtomicBool,
) -> Result<Vec<u32>, String> {
    let (pdf, limits_copy) = (path.to_path_buf(), *limits);
    recognize_with(doc, model, limits, cancel, path, |chunk: Vec<u32>| {
        let (pdf, render) = (pdf.clone(), model.render.clone());
        async move {
            blocking_or_cancel(cancel, move || match render {
                Some(f) => f(&pdf, &chunk, &limits_copy),
                None => rasterize(&pdf, &chunk, &limits_copy),
            })
            .await
        }
    })
    .await
}

/// [`recognize_blank_pages`] with the rasterizer given (tests use a fake one).
pub(crate) async fn recognize_with<F, Fut>(
    doc: &mut Extracted,
    model: &OcrModel,
    limits: &Limits,
    cancel: &AtomicBool,
    path: &Path,
    render: F,
) -> Result<Vec<u32>, String>
where
    F: Fn(Vec<u32>) -> Fut,
    Fut: Future<Output = Result<Vec<RasterPage>, String>>,
{
    let wanted = ocr_page_numbers(&plan_pages(&doc.pages));
    if wanted.is_empty() {
        return Ok(Vec::new());
    }
    if wanted.len() > limits.max_pages {
        return Err(err("rag.ocr_too_many_pages")
            .with("name", extract::file_name(path))
            .num("pages", wanted.len() as u64)
            .num("max", limits.max_pages as u64)
            .into());
    }
    let mut done = Vec::new();
    for chunk in wanted.chunks(BATCH) {
        if cancel.load(Ordering::SeqCst) {
            return Err(super::CANCELLED.into());
        }
        for page in render(chunk.to_vec()).await? {
            if cancel.load(Ordering::SeqCst) {
                return Err(super::CANCELLED.into());
            }
            let text = extract::normalize(&model.recognize(page.png).await?);
            if let Some(slot) = doc.pages.get_mut(page.number as usize - 1) {
                if !text.trim().is_empty() {
                    slot.text = text;
                    done.push(page.number);
                }
            }
        }
    }
    Ok(done)
}

/// The extraction of a knowledge base document: text layers first, then the OCR model (when
/// the base has one) for the PDF pages that have none. Returns the document and the pages
/// that were recognized. Without an OCR model a scanned PDF fails with `rag.pdf_no_text`
/// exactly as before.
pub async fn extract_for_index(
    path: &Path,
    ocr: Option<&OcrModel>,
    limits: &Limits,
    cancel: &AtomicBool,
) -> Result<(Extracted, Vec<u32>), String> {
    let owned = path.to_path_buf();
    let mut doc = blocking_or_cancel(cancel, move || extract::extract_file_raw(&owned)).await?;
    let mut pages = Vec::new();
    if let (Some(model), extract::DocKind::Pdf) = (ocr, doc.kind) {
        pages = recognize_blank_pages(path, &mut doc, model, limits, cancel).await?;
    }
    extract::check_extracted(path, &doc)?;
    Ok((doc, pages))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    fn page(n: u32, text: &str) -> PageText {
        PageText {
            number: Some(n),
            text: text.into(),
        }
    }

    fn pdf(texts: &[&str]) -> Extracted {
        Extracted {
            kind: extract::DocKind::Pdf,
            pages: texts
                .iter()
                .enumerate()
                .map(|(i, t)| page(i as u32 + 1, t))
                .collect(),
        }
    }

    #[test]
    fn text_layer_pages_are_read_and_blank_pages_recognized() {
        let doc = pdf(&["Total 42", "", "  \n ", "Page 4 text", "-", "42"]);
        let plan = plan_pages(&doc.pages);
        assert_eq!(
            plan,
            vec![
                PageSource::TextLayer,
                PageSource::Ocr,
                PageSource::Ocr,
                PageSource::TextLayer,
                PageSource::Ocr,
                PageSource::Ocr,
            ]
        );
        assert_eq!(ocr_page_numbers(&plan), vec![2, 3, 5, 6]);
        assert!(ocr_page_numbers(&plan_pages(&pdf(&["Only text here"]).pages)).is_empty());
    }

    #[test]
    fn limits_come_from_the_environment_values_and_stay_in_range() {
        let d = Limits::parse(None, None, 16);
        assert_eq!((d.max_pages, d.dpi), (16, 150));
        let l = Limits::parse(Some("40"), Some("200"), 16);
        assert_eq!((l.max_pages, l.dpi), (40, 200));
        for bad in ["0", "-3", "many", "5000", ""] {
            assert_eq!(Limits::parse(Some(bad), None, 16).max_pages, 16, "{bad}");
        }
        for bad in ["10", "900", "x"] {
            assert_eq!(Limits::parse(None, Some(bad), 16).dpi, 150, "{bad}");
        }
        assert_eq!(
            Limits::parse(None, None, KB_DEFAULT_MAX_PAGES).max_pages,
            200
        );
    }

    #[test]
    fn rasterizing_needs_the_python_engine_and_says_so() {
        let root = tempfile::tempdir().unwrap();
        let limits = Limits::parse(None, None, 16);
        let e = rasterize_in(root.path(), Path::new("scan.pdf"), &[1], &limits).unwrap_err();
        assert_eq!(e, "ocr.rasterizer_unavailable");
        // Nothing to render needs no environment.
        assert!(rasterize_in(root.path(), Path::new("a.pdf"), &[], &limits)
            .unwrap()
            .is_empty());
    }

    #[test]
    fn rasterizer_answers_are_read_and_checked() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("page-2.png");
        std::fs::write(&file, b"png").unwrap();
        let good = format!(
            "noise\n{}\n",
            serde_json::json!({"ok": true, "page_count": 3, "pages": [{"number": 2, "file": file}]})
        );
        let pages = parse_output(&good, dir.path()).unwrap();
        assert_eq!(
            pages,
            vec![RasterPage {
                number: 2,
                png: b"png".to_vec()
            }]
        );
        // A file outside the rasterizer's folder is never read.
        let other = tempfile::tempdir().unwrap();
        let stray = other.path().join("x.png");
        std::fs::write(&stray, b"x").unwrap();
        let bad = serde_json::json!({"ok": true, "pages": [{"number": 1, "file": stray}]});
        assert!(parse_output(&bad.to_string(), dir.path()).is_err());
        let failed = r#"{"ok": false, "error": "open_failed", "detail": "bad pdf"}"#;
        assert!(parse_output(failed, dir.path())
            .unwrap_err()
            .starts_with("ocr.rasterize_failed"));
        let missing = r#"{"ok": false, "error": "missing_module", "detail": ""}"#;
        assert_eq!(
            parse_output(missing, dir.path()).unwrap_err(),
            "ocr.rasterizer_unavailable"
        );
        assert!(parse_output("", dir.path()).is_err());
    }

    fn fake_model(seen: Arc<Mutex<Vec<usize>>>) -> OcrModel {
        OcrModel::new("ocr-test", move |png| {
            seen.lock().unwrap().push(png.len());
            Box::pin(async move { Ok(format!("scanned text {}", png.len())) })
        })
    }

    fn fake_render(
        log: Arc<Mutex<Vec<Vec<u32>>>>,
    ) -> impl Fn(Vec<u32>) -> std::future::Ready<Result<Vec<RasterPage>, String>> {
        move |chunk: Vec<u32>| {
            log.lock().unwrap().push(chunk.clone());
            std::future::ready(Ok(chunk
                .into_iter()
                .map(|n| RasterPage {
                    number: n,
                    png: vec![0; n as usize],
                })
                .collect()))
        }
    }

    #[tokio::test]
    async fn only_blank_pages_go_to_the_ocr_model_and_are_recorded() {
        let mut doc = pdf(&["First page text", "", "Third page text", ""]);
        let seen = Arc::new(Mutex::new(Vec::new()));
        let rendered = Arc::new(Mutex::new(Vec::new()));
        let done = recognize_with(
            &mut doc,
            &fake_model(seen.clone()),
            &Limits::parse(None, None, 16),
            &AtomicBool::new(false),
            Path::new("mixed.pdf"),
            fake_render(rendered.clone()),
        )
        .await
        .unwrap();
        assert_eq!(done, vec![2, 4]);
        assert_eq!(*rendered.lock().unwrap(), vec![vec![2, 4]]);
        assert_eq!(*seen.lock().unwrap(), vec![2, 4]);
        assert_eq!(doc.pages[0].text, "First page text");
        assert_eq!(doc.pages[1].text, "scanned text 2");
        assert_eq!(doc.pages[3].text, "scanned text 4");
    }

    #[tokio::test]
    async fn pages_are_rendered_in_batches_and_the_page_cap_is_enforced() {
        let mut doc = pdf(&[""; 10]);
        let rendered = Arc::new(Mutex::new(Vec::new()));
        let done = recognize_with(
            &mut doc,
            &fake_model(Arc::default()),
            &Limits {
                max_pages: 10,
                dpi: 150,
            },
            &AtomicBool::new(false),
            Path::new("s.pdf"),
            fake_render(rendered.clone()),
        )
        .await
        .unwrap();
        assert_eq!(done.len(), 10);
        assert_eq!(
            rendered
                .lock()
                .unwrap()
                .iter()
                .map(Vec::len)
                .collect::<Vec<_>>(),
            vec![8, 2]
        );
        let mut doc = pdf(&[""; 10]);
        let e = recognize_with(
            &mut doc,
            &fake_model(Arc::default()),
            &Limits {
                max_pages: 9,
                dpi: 150,
            },
            &AtomicBool::new(false),
            Path::new("s.pdf"),
            fake_render(Arc::default()),
        )
        .await
        .unwrap_err();
        assert!(e.starts_with("rag.ocr_too_many_pages"), "{e}");
    }

    #[tokio::test]
    async fn a_cancelled_run_stops_before_rendering() {
        let mut doc = pdf(&["", ""]);
        let rendered = Arc::new(Mutex::new(Vec::new()));
        let e = recognize_with(
            &mut doc,
            &fake_model(Arc::default()),
            &Limits::parse(None, None, 16),
            &AtomicBool::new(true),
            Path::new("s.pdf"),
            fake_render(rendered.clone()),
        )
        .await
        .unwrap_err();
        assert_eq!(e, "rag.cancelled");
        assert!(rendered.lock().unwrap().is_empty());
    }

    /// A one-page PDF without any text or image.
    fn blank_pdf() -> Vec<u8> {
        let objects = [
            "<< /Type /Catalog /Pages 2 0 R >>".to_string(),
            "<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_string(),
            "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 200] >>".to_string(),
        ];
        let mut pdf = "%PDF-1.4\n".to_string();
        let mut offsets = Vec::new();
        for (i, obj) in objects.iter().enumerate() {
            offsets.push(pdf.len());
            pdf.push_str(&format!("{} 0 obj\n{obj}\nendobj\n", i + 1));
        }
        let xref = pdf.len();
        pdf.push_str(&format!(
            "xref\n0 {}\n0000000000 65535 f \n",
            objects.len() + 1
        ));
        for o in offsets {
            pdf.push_str(&format!("{o:010} 00000 n \n"));
        }
        pdf.push_str(&format!(
            "trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n",
            objects.len() + 1
        ));
        pdf.into_bytes()
    }

    #[tokio::test]
    async fn a_scanned_pdf_without_an_ocr_model_keeps_failing_as_before() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("blank.pdf");
        std::fs::write(&path, blank_pdf()).unwrap();
        let cancel = AtomicBool::new(false);
        let e = extract_for_index(&path, None, &Limits::from_env(), &cancel)
            .await
            .unwrap_err();
        assert!(e.starts_with("rag.pdf_no_text"), "{e}");
    }
}
