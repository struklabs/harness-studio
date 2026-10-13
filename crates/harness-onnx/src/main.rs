//! `harness-onnx`: serves one Hugging Face ONNX encoder model over HTTP.
//!
//! ```text
//! harness-onnx --model <path/to/model.onnx> [--host 127.0.0.1] [--port 8080]
//!              [--alias NAME] [--ort-lib <onnxruntime.dll>] [--provider auto|cpu|directml|cuda]
//!              [--threads N] [--task embedding|rerank|asr_ctc] [--pooling mean|cls|max|last]
//!              [--normalize on|off] [--max-length N] [--batch-size N]
//!              [--genai-lib <onnxruntime-genai.dll>] [--ctx-size N]
//! ```
//!
//! Harness starts it like llama-server: the model file is the `.onnx` of a downloaded repo, the
//! managed ONNX Runtime library comes with `--ort-lib`. A model folder with a
//! `genai_config.json` (Phi-3/4, Qwen, Llama exports for onnxruntime-genai) is served as a chat
//! model through onnxruntime-genai (`--genai-lib`); every other `.onnx` is an encoder.

use harness_onnx::chat_engine::{self, ChatEngine};
use harness_onnx::engine::{self, LoadOptions, Provider};
use harness_onnx::package::{Pooling, Task};
use harness_onnx::server::{self, Shared};
use std::path::PathBuf;

#[derive(Debug)]
struct Args {
    host: String,
    port: u16,
    alias: Option<String>,
    load: LoadOptions,
    /// `onnxruntime-genai.dll` for chat models (default: next to `--ort-lib`).
    genai_lib: Option<PathBuf>,
    /// Tokens of context a chat conversation may use (0: the default).
    ctx_size: usize,
}

const USAGE: &str = "usage: harness-onnx --model <model.onnx> [options] (see --help)";

fn number<T: std::str::FromStr>(flag: &str, v: String) -> Result<T, String> {
    v.parse().map_err(|_| format!("{} must be a number", flag))
}

fn parse_args(args: impl Iterator<Item = String>) -> Result<Args, String> {
    let mut args = args;
    let mut model = None;
    let mut out = Args {
        host: "127.0.0.1".into(),
        port: 8080,
        alias: None,
        load: LoadOptions::new(PathBuf::new()),
        genai_lib: None,
        ctx_size: 0,
    };
    while let Some(flag) = args.next() {
        let mut value = |name: &str| args.next().ok_or_else(|| format!("{} needs a value", name));
        match flag.as_str() {
            "--model" | "-m" => model = Some(PathBuf::from(value("--model")?)),
            "--host" => out.host = value("--host")?,
            "--port" => out.port = number("--port", value("--port")?)?,
            "--alias" | "-a" => out.alias = Some(value("--alias")?),
            "--ort-lib" => out.load.ort_lib = Some(PathBuf::from(value("--ort-lib")?)),
            "--genai-lib" => out.genai_lib = Some(PathBuf::from(value("--genai-lib")?)),
            "--ctx-size" | "-c" => out.ctx_size = number("--ctx-size", value("--ctx-size")?)?,
            "--provider" => {
                let v = value("--provider")?;
                out.load.provider =
                    Provider::parse(&v).ok_or_else(|| format!("unknown provider {}", v))?
            }
            "--threads" => out.load.threads = Some(number("--threads", value("--threads")?)?),
            "--task" => {
                let v = value("--task")?;
                out.load.task = Some(Task::parse(&v).ok_or_else(|| format!("unknown task {}", v))?)
            }
            "--pooling" => {
                let v = value("--pooling")?;
                out.load.pooling =
                    Some(Pooling::parse(&v).ok_or_else(|| format!("unknown pooling {}", v))?)
            }
            "--normalize" => {
                out.load.normalize = Some(match value("--normalize")?.as_str() {
                    "on" | "true" | "1" => true,
                    "off" | "false" | "0" => false,
                    other => return Err(format!("--normalize takes on/off, not {}", other)),
                })
            }
            "--max-length" => {
                out.load.max_length = Some(number("--max-length", value("--max-length")?)?)
            }
            "--batch-size" => out.load.batch_size = number("--batch-size", value("--batch-size")?)?,
            "--help" | "-h" => {
                print!("{}", harness_onnx::cli::HELP);
                std::process::exit(0);
            }
            "--version" | "-V" => {
                println!("{}", harness_onnx::cli::version());
                std::process::exit(0);
            }
            other => return Err(format!("unknown argument {}\n{}", other, USAGE)),
        }
    }
    out.load.model = model.ok_or_else(|| format!("--model is required\n{}", USAGE))?;
    Ok(out)
}

/// The directory of the ONNX Runtime library must be searched for its companions
/// (`DirectML.dll`, `onnxruntime_providers_*.dll`).
#[cfg(windows)]
fn add_dll_directory(dir: &std::path::Path) {
    use std::os::windows::ffi::OsStrExt;
    extern "system" {
        fn SetDllDirectoryW(path: *const u16) -> i32;
    }
    let wide: Vec<u16> = dir.as_os_str().encode_wide().chain(Some(0)).collect();
    // SAFETY: `wide` is a NUL-terminated UTF-16 path that outlives the call.
    unsafe {
        SetDllDirectoryW(wide.as_ptr());
    }
}

#[cfg(not(windows))]
fn add_dll_directory(_dir: &std::path::Path) {}

/// Loads onnxruntime-genai (with the managed ONNX Runtime and DirectML in front of it) and the
/// chat model.
fn load_chat(
    load: &LoadOptions,
    genai_lib: Option<&std::path::Path>,
    package: harness_onnx::genai_pkg::GenaiPackage,
    ctx_size: usize,
) -> Result<ChatEngine, String> {
    let ort = load
        .ort_lib
        .as_deref()
        .ok_or("chat models need --ort-lib (the managed ONNX Runtime)")?;
    let dir = ort.parent().unwrap_or(std::path::Path::new("."));
    let genai_name = if cfg!(windows) {
        "onnxruntime-genai.dll"
    } else if cfg!(target_os = "macos") {
        "libonnxruntime-genai.dylib"
    } else {
        "libonnxruntime-genai.so"
    };
    let genai_lib = genai_lib
        .map(|p| p.to_path_buf())
        .unwrap_or_else(|| dir.join(genai_name));
    if !genai_lib.is_file() {
        return Err(format!(
            "{} is missing: install the ONNX Runtime GenAI library with the Install button in Settings > Installed Runtime",
            genai_lib.display()
        ));
    }
    // DirectML before ONNX Runtime, both by full path, so the managed copies win over the older
    // ones Windows keeps in System32.
    let directml = dir.join("DirectML.dll");
    let mut preload: Vec<&std::path::Path> = Vec::new();
    if directml.is_file() {
        preload.push(&directml);
    }
    preload.push(ort);
    let lib = harness_onnx::genai::Genai::load(&genai_lib, &preload)?;
    let provider = match load.provider {
        Provider::Auto => None,
        Provider::Cpu => Some("cpu"),
        Provider::DirectMl => Some("dml"),
        Provider::Cuda => Some("cuda"),
    };
    ChatEngine::load(&lib, package, provider, ctx_size)
}

fn main() {
    let args = match parse_args(std::env::args().skip(1)) {
        Ok(a) => a,
        Err(e) => {
            eprintln!("{}", e);
            std::process::exit(2);
        }
    };
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .expect("tokio runtime");
    runtime.block_on(async move {
        let alias = args.alias.clone().unwrap_or_else(|| {
            // A genai model is named after its folder (`model.onnx` says nothing).
            let named = chat_engine::package_of(&args.load.model)
                .and_then(|p| p.dir.file_name().map(|n| n.to_string_lossy().to_string()));
            named.unwrap_or_else(|| {
                args.load
                    .model
                    .file_stem()
                    .map(|s| s.to_string_lossy().to_string())
                    .unwrap_or_else(|| "onnx-model".into())
            })
        });
        let shared = Shared::new(alias);
        let listener = match tokio::net::TcpListener::bind((args.host.as_str(), args.port)).await {
            Ok(l) => l,
            Err(e) => {
                eprintln!(
                    "harness-onnx: cannot bind {}:{}: {}",
                    args.host, args.port, e
                );
                std::process::exit(3);
            }
        };
        println!(
            "harness-onnx: listening on http://{}:{}",
            args.host, args.port
        );

        // Loading blocks (disk, graph optimization): off the async threads.
        let loader = shared.clone();
        let load = args.load.clone();
        let (genai_lib, ctx_size) = (args.genai_lib.clone(), args.ctx_size);
        tokio::task::spawn_blocking(move || {
            if let Some(dir) = load.ort_lib.as_ref().and_then(|p| p.parent()) {
                add_dll_directory(dir);
            }
            let started = std::time::Instant::now();
            if let Some(package) = chat_engine::package_of(&load.model) {
                match load_chat(&load, genai_lib.as_deref(), package, ctx_size) {
                    Ok(engine) => {
                        println!(
                            "harness-onnx: chat model ready in {:.1} s: {}",
                            started.elapsed().as_secs_f32(),
                            engine.describe()
                        );
                        loader.set_chat(engine);
                    }
                    Err(e) => {
                        eprintln!("harness-onnx: error: {}", e);
                        std::process::exit(1);
                    }
                }
                return;
            }
            let is_ctc = load.task == Some(Task::AsrCtc)
                || (load.task.is_none() && harness_onnx::asr_ctc::find_root(&load.model).is_some());
            if is_ctc {
                let result = engine::init_runtime(load.ort_lib.as_deref())
                    .and_then(|()| harness_onnx::asr_ctc::load(&load));
                match result {
                    Ok(engine) => {
                        println!(
                            "harness-onnx: CTC model ready in {:.1} s on {}",
                            started.elapsed().as_secs_f32(),
                            engine.provider
                        );
                        loader.set_asr(engine);
                    }
                    Err(e) => {
                        eprintln!("harness-onnx: error: {e}");
                        std::process::exit(1);
                    }
                }
                return;
            }
            let result =
                engine::init_runtime(load.ort_lib.as_deref()).and_then(|()| engine::load(&load));
            match result {
                Ok(engine) => {
                    println!(
                        "harness-onnx: model ready in {:.1} s: {}",
                        started.elapsed().as_secs_f32(),
                        engine.describe()
                    );
                    for note in &engine.package.notes {
                        println!("harness-onnx:   {}", note);
                    }
                    loader.set_engine(engine);
                }
                Err(e) => {
                    eprintln!("harness-onnx: error: {}", e);
                    std::process::exit(1);
                }
            }
        });

        let app = server::router(shared);
        let serve = axum::serve(listener, app).with_graceful_shutdown(async {
            let _ = tokio::signal::ctrl_c().await;
        });
        if let Err(e) = serve.await {
            eprintln!("harness-onnx: server error: {}", e);
            std::process::exit(1);
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(list: &[&str]) -> Result<Args, String> {
        parse_args(list.iter().map(|s| s.to_string()))
    }

    #[test]
    fn parses_a_full_launch() {
        let a = args(&[
            "--model",
            "m/onnx/model.onnx",
            "--port",
            "8123",
            "--alias",
            "minilm",
            "--provider",
            "cpu",
            "--pooling",
            "cls",
            "--normalize",
            "off",
            "--task",
            "rerank",
            "--batch-size",
            "4",
        ])
        .unwrap();
        assert_eq!(a.port, 8123);
        assert_eq!(a.alias.as_deref(), Some("minilm"));
        assert_eq!(a.load.provider, Provider::Cpu);
        assert_eq!(a.load.pooling, Some(Pooling::Cls));
        assert_eq!(a.load.normalize, Some(false));
        assert_eq!(a.load.task, Some(Task::Rerank));
        assert_eq!(a.load.batch_size, 4);
        assert_eq!(a.host, "127.0.0.1");
    }

    #[test]
    fn rejects_missing_model_and_unknown_flags() {
        assert!(args(&[]).unwrap_err().contains("--model"));
        assert!(args(&["--model", "a.onnx", "--bogus"]).is_err());
        assert!(args(&["--model", "a.onnx", "--port", "x"]).is_err());
        assert!(args(&["--model", "a.onnx", "--provider", "tpu"]).is_err());
    }

    #[test]
    fn parses_ctc_task() {
        let launch = args(&["--model", "speech/onnx/model.onnx", "--task", "asr_ctc"]).unwrap();
        assert_eq!(launch.load.task, Some(Task::AsrCtc));
    }
}
