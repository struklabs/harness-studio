//! One conversion job, start to finish (see the module docs of [`super`] for the stages).

use super::plan::{ConvertPlan, PlanFile};
use super::{arch, env, is_direct, proc, Cancel, ConvertStage, ConvertStatus, Progress, CANCELLED};
use crate::downloader::{hf_refusal, DownloadManager};
use crate::error_key::err;
use crate::storage::StorageManager;
use futures_util::StreamExt;
use regex::Regex;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use tokio::io::AsyncWriteExt;

/// Download attempts per file before the job fails (a dropped connection resumes).
const DOWNLOAD_ATTEMPTS: u32 = 4;

fn models_dir() -> PathBuf {
    crate::config::AppSettings::load().primary_models_dir()
}

/// Work folder of a repo: `<models>/.cache/convert/<owner>__<repo>` (the library skips
/// `.cache`, and it is on the library's drive so the result is moved by rename).
pub(super) fn work_dir(repo_id: &str) -> PathBuf {
    models_dir()
        .join(".cache")
        .join("convert")
        .join(repo_id.replace('/', "__"))
}

/// `<models>/<owner>/<repo>-GGUF`.
pub fn target_dir_in(models: &Path, repo_id: &str) -> PathBuf {
    let (owner, repo) = repo_id.split_once('/').unwrap_or(("", repo_id));
    models.join(owner).join(format!("{repo}-GGUF"))
}

/// `<repo>-<QUANT>.gguf`.
pub fn output_name(repo_id: &str, quant: &str) -> String {
    format!("{}-{}.gguf", repo_name(repo_id), quant)
}

/// `mmproj-<repo>-F16.gguf`.
pub fn mmproj_name(repo_id: &str) -> String {
    format!("mmproj-{}-F16.gguf", repo_name(repo_id))
}

fn repo_name(repo_id: &str) -> &str {
    repo_id.rsplit('/').next().unwrap_or(repo_id)
}

/// What a finished conversion produced.
struct Done {
    output: PathBuf,
    mmproj: Option<PathBuf>,
    warning: Option<String>,
}

/// Runs the job to its end and records how it ended.
pub(super) async fn drive(id: u64, plan: ConvertPlan, cancel: Cancel) {
    let repo = plan.repo_id.clone();
    if cancel.is_set() {
        finish_unsuccessful(id, &repo, CANCELLED.to_string());
        return;
    }
    super::update(id, |j| {
        j.status = ConvertStatus::Running;
        j.stage = ConvertStage::Preparing;
    });
    match execute(id, &plan, &cancel).await {
        Ok(done) => super::update(id, |j| {
            j.status = ConvertStatus::Completed;
            j.stage = ConvertStage::Done;
            j.progress = Some(1.0);
            j.detail = String::new();
            j.output = Some(done.output.display().to_string());
            j.mmproj = done.mmproj.map(|p| p.display().to_string());
            j.warning = done.warning;
        }),
        Err(e) => finish_unsuccessful(id, &repo, e),
    }
}

fn finish_unsuccessful(id: u64, repo_id: &str, error: String) {
    let work = work_dir(repo_id);
    // The 16-bit intermediate is never worth keeping; the downloaded files are (a retry
    // resumes from them) until the job is removed.
    let _ = std::fs::remove_dir_all(work.join("out"));
    let kept = env::dir_size(&work.join("src"));
    let cancelled = error == CANCELLED;
    super::update(id, |j| {
        j.status = if cancelled {
            ConvertStatus::Cancelled
        } else {
            ConvertStatus::Failed
        };
        j.error = (!cancelled).then_some(error);
        j.kept_bytes = kept;
        j.progress = None;
    });
}

fn check_cancel(cancel: &Cancel) -> Result<(), String> {
    if cancel.is_set() {
        Err(CANCELLED.to_string())
    } else {
        Ok(())
    }
}

async fn execute(id: u64, plan: &ConvertPlan, cancel: &Cancel) -> Result<Done, String> {
    let repo = plan.repo_id.as_str();
    let quant = super::job(id)
        .map(|j| j.quant)
        .unwrap_or_else(|| super::DEFAULT_QUANT.to_string());
    let keep_original = super::job(id).is_some_and(|j| j.keep_original);

    // --- the tools -------------------------------------------------------------------------
    let tag = super::runtime_tag().ok_or_else(|| String::from(err("convert.runtime_missing")))?;
    let quantize = if is_direct(&quant) {
        None
    } else {
        let bin = super::quantize_binary()?;
        let known = {
            let server = super::runtime_dir().join(crate::runtime::binary_name());
            tokio::task::spawn_blocking(move || crate::runtimes::quant::detect_quant_types(&server))
                .await
                .map_err(|e| e.to_string())?
        };
        if let Some(list) = known.filter(|l| !l.iter().any(|q| q.eq_ignore_ascii_case(&quant))) {
            return Err(err("convert.invalid_quant")
                .with("quant", &quant)
                .with("allowed", list.join(", "))
                .into());
        }
        Some(bin)
    };

    // --- 1. the environment ----------------------------------------------------------------
    let progress = Progress::new(move |p| {
        super::update(id, |j| {
            j.stage = ConvertStage::Preparing;
            j.detail = if p.detail.is_empty() {
                p.step.clone()
            } else {
                format!("{}: {}", p.step, p.detail)
            };
            j.progress = (p.total_bytes > 0)
                .then(|| (p.downloaded_bytes as f32 / p.total_bytes as f32).min(1.0));
        })
    });
    let env_root = env::root();
    env::ensure_in(&env_root, &tag, cancel, &progress).await?;
    check_cancel(cancel)?;

    // The converter's own list is the last word (the plan may have been made without it).
    let converter_dir = env::source_dir_in(&env_root, &tag);
    let archs = arch::scan_dir(&converter_dir).map_err(|e| e.to_string())?;
    if let Some(a) = &plan.architecture {
        if !archs.supports_text(a) {
            return Err(err("convert.unsupported_architecture")
                .with("architecture", a)
                .with("repo", repo)
                .into());
        }
    }
    let vision = plan.task.as_deref() == Some("image-text-to-text")
        && plan
            .architecture
            .as_deref()
            .is_some_and(|a| archs.supports_vision(a));

    // --- 2. the weights --------------------------------------------------------------------
    let work = work_dir(repo);
    let src = work.join("src");
    let out = work.join("out");
    let _ = std::fs::remove_dir_all(&out);
    for dir in [&src, &out] {
        std::fs::create_dir_all(dir).map_err(|e| {
            String::from(
                err("fs.create_dir_failed")
                    .with("path", dir.display())
                    .with("error", e),
            )
        })?;
    }
    let (free, _) = StorageManager::get_directory_disk_space(&work);
    let have = env::dir_size(&src);
    let need = plan.estimate(&quant).disk_bytes;
    if free + have < need {
        return Err(err("convert.not_enough_disk")
            .with("need", crate::doctor::fmt_bytes(need))
            .with("free", crate::doctor::fmt_bytes(free + have))
            .with("path", work.display())
            .into());
    }
    download_all(id, plan, &src, cancel).await?;
    check_cancel(cancel)?;

    // --- 3. conversion to a 16-bit GGUF ----------------------------------------------------
    let python = env::python_bin_in(&env_root);
    let outtype = match quant.as_str() {
        "F16" => "f16",
        "BF16" => "bf16",
        _ => "auto",
    };
    let name = repo_name(repo).to_string();
    set_stage(id, ConvertStage::Converting, "convert_hf_to_gguf.py");
    let mut cmd = converter_command(&python, &converter_dir, &env_root);
    cmd.arg(&src)
        .args(["--outtype", outtype])
        .arg("--outfile")
        .arg(out.join(format!("{name}-{{ftype}}.gguf")));
    run_python(id, cmd, cancel, "convert.conversion_failed", repo).await?;
    let produced = find_gguf(&out, false)?;

    let mut warning = None;
    let mut mmproj_file = None;
    if vision {
        check_cancel(cancel)?;
        set_stage(
            id,
            ConvertStage::Converting,
            "convert_hf_to_gguf.py --mmproj",
        );
        let mut cmd = converter_command(&python, &converter_dir, &env_root);
        cmd.arg(&src)
            .args(["--mmproj", "--outtype", "f16"])
            .arg("--outfile")
            .arg(out.join(format!("mmproj-{name}-{{ftype}}.gguf")));
        match run_python(id, cmd, cancel, "convert.mmproj_failed", repo).await {
            Ok(()) => mmproj_file = find_gguf(&out, true).ok(),
            Err(e) if e == CANCELLED => return Err(e),
            Err(e) => warning = Some(e),
        }
    }
    check_cancel(cancel)?;

    // --- 4. quantization -------------------------------------------------------------------
    let result_tmp = out.join(output_name(repo, &quant));
    match quantize {
        Some(bin) => {
            set_stage(id, ConvertStage::Quantizing, "llama-quantize");
            let mut cmd = tokio::process::Command::new(&bin);
            cmd.current_dir(bin.parent().unwrap_or(Path::new(".")))
                .arg(&produced)
                .arg(&result_tmp)
                .arg(&quant);
            let outcome = proc::run(cmd, &cancel.flag(), |line| {
                if let Some((n, total)) = parse_quantize_step(line) {
                    super::update(id, |j| {
                        j.progress = Some(n as f32 / total.max(1) as f32);
                        j.detail = line_tensor(line);
                    });
                }
            })
            .await
            .map_err(|e| {
                String::from(
                    err("convert.spawn_failed")
                        .with("step", "quantize")
                        .with("error", e),
                )
            })?;
            if outcome.cancelled {
                return Err(CANCELLED.to_string());
            }
            if !outcome.success || !result_tmp.is_file() {
                return Err(err("convert.quantize_failed")
                    .with("quant", &quant)
                    .with("log", outcome.log())
                    .into());
            }
            let _ = std::fs::remove_file(&produced);
        }
        None => move_file(&produced, &result_tmp)?,
    }
    check_cancel(cancel)?;

    // --- 5. into the library ---------------------------------------------------------------
    set_stage(id, ConvertStage::Finalizing, "");
    let models = models_dir();
    let target = target_dir_in(&models, repo);
    std::fs::create_dir_all(&target).map_err(|e| {
        String::from(
            err("fs.create_dir_failed")
                .with("path", target.display())
                .with("error", e),
        )
    })?;
    let final_path = target.join(output_name(repo, &quant));
    check_gguf(&result_tmp)?;
    move_file(&result_tmp, &final_path)?;
    let mmproj = match mmproj_file {
        Some(file) => {
            let dest = target.join(mmproj_name(repo));
            move_file(&file, &dest)?;
            Some(dest)
        }
        None => None,
    };
    if keep_original {
        keep_source(&src, &models.join(repo))?;
    }
    let _ = std::fs::remove_dir_all(&work);
    Ok(Done {
        output: final_path,
        mmproj,
        warning,
    })
}

fn set_stage(id: u64, stage: ConvertStage, detail: &str) {
    super::update(id, |j| {
        j.stage = stage;
        j.progress = None;
        j.detail = detail.to_string();
    });
}

fn converter_command(
    python: &Path,
    converter_dir: &Path,
    env_root: &Path,
) -> tokio::process::Command {
    let mut cmd = tokio::process::Command::new(python);
    cmd.current_dir(converter_dir)
        .arg("convert_hf_to_gguf.py")
        .envs(env::python_env(env_root));
    cmd
}

/// Runs one converter command, turning its progress lines into job progress.
async fn run_python(
    id: u64,
    cmd: tokio::process::Command,
    cancel: &Cancel,
    error_key: &'static str,
    repo: &str,
) -> Result<(), String> {
    let outcome = proc::run(cmd, &cancel.flag(), |line| match parse_line(line) {
        Line::Bar(fraction) => super::update(id, |j| j.progress = Some(fraction)),
        Line::Info(text) => super::update(id, |j| j.detail = text),
        Line::Other => {}
    })
    .await
    .map_err(|e| {
        String::from(
            err("convert.spawn_failed")
                .with("step", "convert")
                .with("error", e),
        )
    })?;
    if outcome.cancelled {
        return Err(CANCELLED.to_string());
    }
    if outcome.success {
        return Ok(());
    }
    let log = outcome.log();
    if let Some(arch) = unsupported_architecture_in(&log) {
        return Err(err("convert.unsupported_architecture")
            .with("architecture", arch)
            .with("repo", repo)
            .into());
    }
    Err(match error_key {
        "convert.mmproj_failed" => err("convert.mmproj_failed").with("log", log).into(),
        _ => err("convert.conversion_failed").with("log", log).into(),
    })
}

// --- output lines ------------------------------------------------------------------------------

/// What one converter output line means for the job.
#[derive(Debug, Clone, PartialEq)]
pub enum Line {
    /// A progress bar (`Writing:  21%|##...`): the fraction done.
    Bar(f32),
    /// A log line worth showing as the current activity.
    Info(String),
    Other,
}

/// Reads a converter line: tqdm bars give a fraction, `INFO:hf-to-gguf:` lines the activity.
pub fn parse_line(line: &str) -> Line {
    static BAR: OnceLock<Regex> = OnceLock::new();
    let bar = BAR.get_or_init(|| Regex::new(r"(\d{1,3})%\|").expect("static regex"));
    if let Some(c) = bar.captures(line) {
        let pct: f32 = c[1].parse().unwrap_or(0.0);
        return Line::Bar((pct / 100.0).clamp(0.0, 1.0));
    }
    for prefix in ["INFO:hf-to-gguf:", "INFO:gguf.gguf_writer:"] {
        if let Some(rest) = line.strip_prefix(prefix) {
            let rest = rest.trim();
            if !rest.is_empty() {
                return Line::Info(rest.chars().take(160).collect());
            }
        }
    }
    Line::Other
}

/// `[ 271/ 272] blk.29.ffn_norm.weight ...` of `llama-quantize` -> (271, 272).
pub fn parse_quantize_step(line: &str) -> Option<(u32, u32)> {
    static STEP: OnceLock<Regex> = OnceLock::new();
    let re = STEP.get_or_init(|| Regex::new(r"^\[\s*(\d+)/\s*(\d+)\]").expect("static regex"));
    let c = re.captures(line.trim_start())?;
    Some((c[1].parse().ok()?, c[2].parse().ok()?))
}

fn line_tensor(line: &str) -> String {
    line.split_once(']')
        .map(|(_, rest)| rest.trim())
        .and_then(|rest| rest.split_whitespace().next())
        .unwrap_or("")
        .to_string()
}

/// The architecture a converter refused: `NotImplementedError: Architecture 'X' not supported!`.
pub fn unsupported_architecture_in(log: &str) -> Option<String> {
    let re = Regex::new(r"Architecture '?([A-Za-z0-9_.]+)'? not supported").expect("static regex");
    re.captures(log).map(|c| c[1].to_string())
}

// --- files ---------------------------------------------------------------------------------------

/// The GGUF a converter run wrote into `dir` (the biggest `.gguf`; `mmproj` picks the
/// projector, otherwise the projector is skipped).
fn find_gguf(dir: &Path, mmproj: bool) -> Result<PathBuf, String> {
    let mut best: Option<(u64, PathBuf)> = None;
    if let Ok(rd) = std::fs::read_dir(dir) {
        for e in rd.flatten() {
            let path = e.path();
            let name = e.file_name().to_string_lossy().to_ascii_lowercase();
            if !name.ends_with(".gguf") || name.starts_with("mmproj") != mmproj {
                continue;
            }
            let size = e.metadata().map(|m| m.len()).unwrap_or(0);
            if best.as_ref().is_none_or(|(s, _)| size > *s) {
                best = Some((size, path));
            }
        }
    }
    best.map(|(_, p)| p).ok_or_else(|| {
        err("convert.output_missing")
            .with("path", dir.display())
            .into()
    })
}

/// The file starts with the GGUF magic and is not empty.
fn check_gguf(path: &Path) -> Result<(), String> {
    use std::io::Read;
    let mut magic = [0u8; 4];
    let ok = std::fs::File::open(path)
        .and_then(|mut f| f.read_exact(&mut magic))
        .is_ok()
        && &magic == b"GGUF";
    if ok {
        Ok(())
    } else {
        Err(err("convert.output_missing")
            .with("path", path.display())
            .into())
    }
}

/// Renames `from` to `to` (replacing it); copies when they are on different drives.
pub fn move_file(from: &Path, to: &Path) -> Result<(), String> {
    let _ = std::fs::remove_file(to);
    if std::fs::rename(from, to).is_ok() {
        return Ok(());
    }
    std::fs::copy(from, to).map_err(|e| {
        String::from(
            err("fs.write_failed")
                .with("path", to.display())
                .with("error", e),
        )
    })?;
    let _ = std::fs::remove_file(from);
    Ok(())
}

/// Moves the downloaded safetensors folder to `dest` (files already there are replaced).
fn keep_source(src: &Path, dest: &Path) -> Result<(), String> {
    std::fs::create_dir_all(dest).map_err(|e| {
        String::from(
            err("fs.create_dir_failed")
                .with("path", dest.display())
                .with("error", e),
        )
    })?;
    if let Ok(rd) = std::fs::read_dir(src) {
        for e in rd.flatten() {
            if e.path().is_file() {
                move_file(&e.path(), &dest.join(e.file_name()))?;
            }
        }
    }
    Ok(())
}

// --- download ------------------------------------------------------------------------------------

fn resolve_url(repo_id: &str, file: &str) -> Result<reqwest::Url, String> {
    let mut url = reqwest::Url::parse("https://huggingface.co/")
        .map_err(|e| String::from(err("hf.url_build_failed").with("error", e)))?;
    {
        let mut seg = url
            .path_segments_mut()
            .map_err(|_| String::from(err("hf.url_build_failed").with("error", repo_id)))?;
        for s in repo_id.split('/') {
            seg.push(s);
        }
        seg.push("resolve").push("main").push(file);
    }
    Ok(url)
}

async fn download_all(
    id: u64,
    plan: &ConvertPlan,
    dir: &Path,
    cancel: &Cancel,
) -> Result<(), String> {
    let token = crate::hf_auth::current_token().await;
    let client = reqwest::Client::builder()
        .user_agent("HarnessStudio/0.1.0")
        .connect_timeout(std::time::Duration::from_secs(30))
        .build()
        .map_err(|e| crate::http_error::transport(&e, "HTTP client"))?;
    let total: u64 = plan.files.iter().map(|f| f.size).sum();
    super::update(id, |j| {
        j.stage = ConvertStage::Downloading;
        j.total_bytes = total;
        j.downloaded_bytes = 0;
        j.progress = Some(0.0);
    });
    let mut base = 0u64;
    for file in &plan.files {
        check_cancel(cancel)?;
        download_file(
            id,
            &client,
            plan,
            file,
            dir,
            base,
            total,
            token.as_deref(),
            cancel,
        )
        .await?;
        base += file.size;
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
async fn download_file(
    id: u64,
    client: &reqwest::Client,
    plan: &ConvertPlan,
    file: &PlanFile,
    dir: &Path,
    base: u64,
    total: u64,
    token: Option<&str>,
    cancel: &Cancel,
) -> Result<(), String> {
    if file.name.contains(['/', '\\']) || file.name.contains("..") {
        return Err(err("downloads.invalid_file_name")
            .with("name", &file.name)
            .into());
    }
    let dest = dir.join(&file.name);
    // A file only gets its final name once it is complete and checked.
    if file.size > 0 && std::fs::metadata(&dest).is_ok_and(|m| m.len() == file.size) {
        report_bytes(id, base + file.size, total, &file.name);
        return Ok(());
    }
    let part = dir.join(format!("{}.part", file.name));
    let url = resolve_url(&plan.repo_id, &file.name)?;
    let token_present = token.is_some_and(|t| !t.trim().is_empty());
    let mut last_error = String::new();
    for _attempt in 0..DOWNLOAD_ATTEMPTS {
        check_cancel(cancel)?;
        let have = std::fs::metadata(&part).map(|m| m.len()).unwrap_or(0);
        let mut req = DownloadManager::apply_hf_auth(client.get(url.clone()), url.as_str(), token);
        if have > 0 {
            req = req.header("Range", format!("bytes={have}-"));
        }
        let res = match req.send().await {
            Ok(r) => r,
            Err(e) => {
                last_error = crate::http_error::transport(&e, url.as_str());
                continue;
            }
        };
        let status = res.status();
        if let Some(reason) = hf_refusal(status, res.headers(), &plan.repo_id, token_present) {
            return Err(reason);
        }
        if status == reqwest::StatusCode::RANGE_NOT_SATISFIABLE {
            // The part is already complete or garbage: start over.
            let _ = std::fs::remove_file(&part);
            continue;
        }
        if !status.is_success() {
            return Err(err("http.status")
                .with("status", status)
                .with("url", url.as_str())
                .into());
        }
        let resumed = status == reqwest::StatusCode::PARTIAL_CONTENT && have > 0;
        let write_err = |e: std::io::Error| {
            String::from(
                err("fs.write_failed")
                    .with("path", part.display())
                    .with("error", e),
            )
        };
        let mut out = if resumed {
            tokio::fs::OpenOptions::new()
                .append(true)
                .open(&part)
                .await
                .map_err(write_err)?
        } else {
            tokio::fs::File::create(&part).await.map_err(write_err)?
        };
        let mut got = if resumed { have } else { 0 };
        let mut stream = res.bytes_stream();
        let mut broken = false;
        while let Some(chunk) = stream.next().await {
            check_cancel(cancel)?;
            match chunk {
                Ok(bytes) => {
                    out.write_all(&bytes).await.map_err(write_err)?;
                    got += bytes.len() as u64;
                    report_bytes(id, base + got, total, &file.name);
                }
                Err(e) => {
                    last_error = crate::http_error::transport(&e, url.as_str());
                    broken = true;
                    break;
                }
            }
        }
        out.flush().await.map_err(write_err)?;
        drop(out);
        if broken {
            continue;
        }
        let size = std::fs::metadata(&part).map(|m| m.len()).unwrap_or(0);
        if file.size > 0 && size != file.size {
            last_error = err("convert.download_incomplete")
                .with("file", &file.name)
                .with("got", size)
                .with("expected", file.size)
                .into();
            continue;
        }
        if let Some(expected) = &file.sha256 {
            let p = part.clone();
            let actual = tokio::task::spawn_blocking(move || crate::downloader::sha256_file(&p))
                .await
                .map_err(|e| e.to_string())??;
            if &actual != expected {
                let _ = std::fs::remove_file(&part);
                return Err(err("runtime.digest_mismatch")
                    .with("file", &file.name)
                    .with("expected", expected)
                    .with("actual", actual)
                    .into());
            }
        }
        let _ = std::fs::remove_file(&dest);
        std::fs::rename(&part, &dest).map_err(|e| {
            String::from(
                err("fs.rename_failed")
                    .with("from", part.display())
                    .with("to", dest.display())
                    .with("error", e),
            )
        })?;
        return Ok(());
    }
    Err(if last_error.is_empty() {
        err("convert.download_incomplete")
            .with("file", &file.name)
            .with("got", 0)
            .with("expected", file.size)
            .into()
    } else {
        last_error
    })
}

fn report_bytes(id: u64, downloaded: u64, total: u64, name: &str) {
    super::update(id, |j| {
        j.downloaded_bytes = downloaded;
        j.progress = (total > 0).then(|| (downloaded as f32 / total as f32).min(1.0));
        j.detail = name.to_string();
    });
}
