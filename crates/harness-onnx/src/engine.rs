//! The model: ONNX Runtime session + tokenizer + the pooling rules of the repo.
//!
//! * [`load`] picks the execution provider (DirectML, CUDA, CPU), builds the session and
//!   checks that the graph's inputs are ones this server knows how to feed.
//! * [`Engine::embed`] tokenizes, runs the graph in length-sorted batches, pools token vectors
//!   per [`ModelPackage::pooling`] (unless the graph already outputs a sentence vector) and
//!   normalizes.
//! * [`Engine::rerank`] encodes `(query, document)` pairs and returns the classifier logit.
//! * [`Engine::classify`], [`Engine::zero_shot`], [`Engine::token_classify`],
//!   [`Engine::fill_mask`] and [`Engine::answer`] run the task heads (logits read by
//!   [`crate::tasks`]).

use crate::api::ApiError;
use crate::package::{ModelPackage, Pooling, Task};
use crate::pooling;
use crate::tasks::{self, Answer, Entity, LabelScore, TokenInfo};
use ort::ep::ExecutionProvider as _;
use ort::session::Session;
use ort::value::{Tensor, TensorElementType};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, OnceLock};
use tokenizers::{
    EncodeInput, Tokenizer, TruncationDirection, TruncationParams, TruncationStrategy,
};

/// Which execution provider to use.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Provider {
    /// DirectML on Windows, CUDA where the runtime has it, else CPU.
    Auto,
    Cpu,
    DirectMl,
    Cuda,
}

impl Provider {
    pub fn parse(s: &str) -> Option<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "auto" | "" => Some(Self::Auto),
            "cpu" => Some(Self::Cpu),
            "directml" | "dml" => Some(Self::DirectMl),
            "cuda" => Some(Self::Cuda),
            _ => None,
        }
    }
}

/// How the server was asked to load the model.
#[derive(Debug, Clone)]
pub struct LoadOptions {
    pub model: PathBuf,
    /// `onnxruntime.dll` / `libonnxruntime.so` of the managed runtime. `None`: the default
    /// name, looked up by the OS (`ORT_DYLIB_PATH` is honoured).
    pub ort_lib: Option<PathBuf>,
    pub provider: Provider,
    pub threads: Option<usize>,
    /// Overrides of what the repo files say (`--task`, `--pooling`, `--max-length`).
    pub task: Option<Task>,
    pub pooling: Option<Pooling>,
    pub normalize: Option<bool>,
    pub max_length: Option<usize>,
    /// Texts per graph run.
    pub batch_size: usize,
}

impl LoadOptions {
    pub fn new(model: PathBuf) -> Self {
        Self {
            model,
            ort_lib: None,
            provider: Provider::Auto,
            threads: None,
            task: None,
            pooling: None,
            normalize: None,
            max_length: None,
            batch_size: 16,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum IdType {
    Int64,
    Int32,
}

/// How the graph's inputs are named.
#[derive(Debug, Clone)]
struct Inputs {
    ids: (String, IdType),
    mask: Option<(String, IdType)>,
    type_ids: Option<(String, IdType)>,
    position_ids: Option<(String, IdType)>,
}

/// Which output carries the result (`extra`: the second one of a QA head, `end_logits`).
#[derive(Debug, Clone)]
struct OutputSpec {
    name: String,
    extra: Option<String>,
}

/// Context windows of one question-answering request, at most: a longer context is refused
/// (each window is a graph run under the session lock).
pub const MAX_QA_WINDOWS: usize = 32;

/// All graph runs take this lock, including DirectML sessions.
pub(crate) type SessionLock = Mutex<Session>;

pub struct Engine {
    session: SessionLock,
    /// Answers a request the GPU session turned into non-finite values.
    cpu: CpuFallback,
    tokenizer: Tokenizer,
    /// Question answering: the tokenizer without truncation; [`Engine::answer`] cuts the
    /// context into overlapping windows (`doc_stride`) itself, the question whole in each,
    /// like the transformers pipeline.
    qa_tokenizer: Option<Tokenizer>,
    pub package: ModelPackage,
    inputs: Inputs,
    output: OutputSpec,
    pad_id: u32,
    /// `DirectML`, `CUDA` or `CPU`.
    pub provider: String,
    batch_size: usize,
    /// Token budget of one graph run (batch x longest text).
    token_budget: usize,
}

/// Loads the ONNX Runtime library (once per process). Call before [`load`].
pub fn init_runtime(lib: Option<&Path>) -> Result<(), String> {
    let builder = match lib {
        Some(path) => ort::init_from(path)
            .map_err(|e| format!("cannot load ONNX Runtime from {}: {}", path.display(), e))?,
        None => ort::init_from(default_lib_name()).map_err(|e| {
            format!(
                "cannot load ONNX Runtime ({}): {}",
                default_lib_name().display(),
                e
            )
        })?,
    };
    builder.with_name("harness-onnx").commit();
    Ok(())
}

fn default_lib_name() -> PathBuf {
    if let Some(p) = std::env::var_os("ORT_DYLIB_PATH") {
        return PathBuf::from(p);
    }
    PathBuf::from(if cfg!(windows) {
        "onnxruntime.dll"
    } else if cfg!(target_os = "macos") {
        "libonnxruntime.dylib"
    } else {
        "libonnxruntime.so"
    })
}

fn id_type(t: Option<TensorElementType>) -> Option<IdType> {
    match t? {
        TensorElementType::Int64 => Some(IdType::Int64),
        TensorElementType::Int32 => Some(IdType::Int32),
        _ => None,
    }
}

fn classify_inputs(session: &Session) -> Result<Inputs, String> {
    let mut ids = None;
    let mut mask = None;
    let mut type_ids = None;
    let mut position_ids = None;
    let mut unknown = Vec::new();
    for input in session.inputs() {
        let name = input.name().to_string();
        let ty = id_type(input.dtype().tensor_type());
        let entry = ty.map(|t| (name.clone(), t));
        match (name.as_str(), entry) {
            ("input_ids", Some(e)) => ids = Some(e),
            ("attention_mask", Some(e)) => mask = Some(e),
            ("token_type_ids", Some(e)) => type_ids = Some(e),
            ("position_ids", Some(e)) => position_ids = Some(e),
            _ => unknown.push(name),
        }
    }
    if !unknown.is_empty() {
        return Err(format!(
            "the graph needs inputs this server cannot feed: {} (supported: input_ids, attention_mask, token_type_ids, position_ids as int32/int64)",
            unknown.join(", ")
        ));
    }
    Ok(Inputs {
        ids: ids.ok_or("the graph has no input_ids input (not a text encoder)")?,
        mask,
        type_ids,
        position_ids,
    })
}

/// Output to read: a ready sentence vector when the graph makes one, else the token vectors.
fn pick_output(session: &Session, task: Task) -> Result<OutputSpec, String> {
    let names: Vec<&str> = session.outputs().iter().map(|o| o.name()).collect();
    if names.is_empty() {
        return Err("the graph has no outputs".into());
    }
    let preference: &[&str] = match task {
        Task::Embedding => &[
            "sentence_embedding",
            "dense_vecs",
            "last_hidden_state",
            "token_embeddings",
        ],
        Task::Rerank | Task::Classify | Task::TokenClassify | Task::FillMask => {
            &["logits", "scores", "score"]
        }
        Task::QuestionAnswering => {
            let has = |n: &str| names.contains(&n);
            if !(has("start_logits") && has("end_logits")) {
                return Err(format!(
                    "a question-answering graph needs start_logits and end_logits outputs (has: {})",
                    names.join(", ")
                ));
            }
            return Ok(OutputSpec {
                name: "start_logits".into(),
                extra: Some("end_logits".into()),
            });
        }
        Task::AsrCtc => return Err("CTC models use asr_ctc::load".into()),
    };
    let name = preference
        .iter()
        .find(|p| names.contains(p))
        .copied()
        .unwrap_or(names[0]);
    Ok(OutputSpec {
        name: name.to_string(),
        extra: None,
    })
}

/// Set by `--cuda-fallback-exit`: a CUDA provider that cannot be used ends the process
/// ([`crate::cli::EXIT_CUDA_UNUSABLE`]) instead of running on the CPU.
static CUDA_FALLBACK_EXIT: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(false);

pub fn set_cuda_fallback_exit(on: bool) {
    CUDA_FALLBACK_EXIT.store(on, std::sync::atomic::Ordering::SeqCst);
}

/// With `--cuda-fallback-exit`: says why CUDA is not usable and exits for Harness to restart
/// the model on DirectML.
fn cuda_unusable(why: &str) {
    if CUDA_FALLBACK_EXIT.load(std::sync::atomic::Ordering::SeqCst) {
        eprintln!("harness-onnx: CUDA is not usable ({why}); exiting so that Harness starts the model on DirectML");
        std::process::exit(crate::cli::EXIT_CUDA_UNUSABLE);
    }
}

/// Operators of dynamically quantized graphs that crash DirectML.
const DML_UNSAFE_OPS: [&str; 2] = ["ConvInteger", "MatMulInteger"];

fn register_provider(
    builder: &mut ort::session::builder::SessionBuilder,
    choice: Provider,
    skip_dml: bool,
) -> &'static str {
    let wants_dml =
        matches!(choice, Provider::Auto | Provider::DirectMl) && cfg!(windows) && !skip_dml;
    let wants_cuda = matches!(choice, Provider::Auto | Provider::Cuda);
    // A provider that was asked for by name and does not load says why (missing CUDA / cuDNN
    // libraries, a runtime build without it); `auto` falls through quietly.
    let explain = |name: &str, why: String| {
        if choice != Provider::Auto {
            eprintln!("harness-onnx: {name} is not usable, running on the CPU: {why}");
        }
    };
    if wants_dml {
        let ep = ort::ep::DirectML::default();
        match ep.is_available() {
            Ok(true) => match ep.register(builder) {
                Ok(()) => return "DirectML",
                Err(e) => explain("DirectML", e.to_string()),
            },
            Ok(false) => explain("DirectML", "this ONNX Runtime build has no DirectML".into()),
            Err(e) => explain("DirectML", e.to_string()),
        }
    }
    if wants_cuda {
        let ep = ort::ep::CUDA::default();
        match ep.is_available() {
            Ok(true) => match ep.register(builder) {
                Ok(()) => return "CUDA",
                Err(e) => {
                    cuda_unusable(&e.to_string());
                    explain("CUDA", e.to_string())
                }
            },
            Ok(false) => {
                cuda_unusable("this ONNX Runtime build has no CUDA");
                explain("CUDA", "this ONNX Runtime build has no CUDA".into())
            }
            Err(e) => {
                cuda_unusable(&e.to_string());
                explain("CUDA", e.to_string())
            }
        }
    }
    "CPU"
}

pub(crate) fn build_session(
    model: &Path,
    choice: Provider,
    threads: Option<usize>,
) -> Result<(Session, &'static str), String> {
    build_session_with(model, choice, threads, true)
}

/// `profile`: honour `HARNESS_ONNX_PROFILE`. The CPU retry of a failed GPU session and the
/// lazy CPU session of a runtime retry are not profiled.
pub(crate) fn build_session_with(
    model: &Path,
    choice: Provider,
    threads: Option<usize>,
    profile: bool,
) -> Result<(Session, &'static str), String> {
    // DirectML crashes the process (an access violation, not an error) on the integer
    // operators of dynamically quantized exports (transformers.js `model_quantized`): the
    // convolutions of vision and CTC speech models, the matrix products of text encoders (BERT,
    // DistilBERT: seen on an NVIDIA GPU). Such a graph runs on the CPU, or on CUDA when `auto`
    // finds it.
    let integer_op = if matches!(choice, Provider::Auto | Provider::DirectMl) && cfg!(windows) {
        crate::package::graph_uses_any_op(model, &DML_UNSAFE_OPS)
    } else {
        None
    };
    let skip_dml = integer_op.is_some();
    if let (Some(op), Provider::DirectMl) = (integer_op, choice) {
        eprintln!(
            "harness-onnx: DirectML cannot run {op} (an INT8 export), running on the CPU; the FP32 or FP16 file runs on the GPU"
        );
    }
    let make = |choice: Provider| -> Result<(Session, &'static str), String> {
        let mut builder = Session::builder().map_err(|e| e.to_string())?;
        if let Some(n) = threads {
            builder = builder.with_intra_threads(n).map_err(|e| e.to_string())?;
        }
        if let Some(path) = profile_target(std::env::var_os("HARNESS_ONNX_PROFILE"), profile) {
            builder = builder
                .with_profiling(Path::new(&path))
                .map_err(|e| e.to_string())?;
        }
        // DirectML needs sequential execution and no memory pattern (ORT's own requirement).
        let name = register_provider(&mut builder, choice, skip_dml);
        if name == "DirectML" {
            builder = builder
                .with_memory_pattern(false)
                .map_err(|e| e.to_string())?;
            builder = builder
                .with_parallel_execution(false)
                .map_err(|e| e.to_string())?;
        }
        let session = builder.commit_from_file(model).map_err(|e| {
            if name == "CUDA" {
                cuda_unusable(&e.to_string());
            }
            format!("cannot load {} on {}: {}", model.display(), name, e)
        })?;
        Ok((session, name))
    };
    match make(choice) {
        Ok(ok) => Ok(ok),
        // A GPU provider that registered but cannot build the graph: the CPU still can.
        Err(first) if choice != Provider::Cpu => match make(Provider::Cpu) {
            Ok(ok) => {
                eprintln!("harness-onnx: {} (falling back to the CPU)", first);
                Ok(ok)
            }
            Err(_) => Err(first),
        },
        Err(e) => Err(e),
    }
}

/// The profile file a session writes: the `HARNESS_ONNX_PROFILE` value, for a session that
/// is profiled (not the CPU retry of a failed GPU warm-up).
pub(crate) fn profile_target(
    env: Option<std::ffi::OsString>,
    profiled: bool,
) -> Option<std::ffi::OsString> {
    env.filter(|_| profiled)
}

/// Whether a failed warm-up of a GPU session is retried on the CPU: the graph did not run,
/// or ran into non-finite values.
pub(crate) fn warmup_falls_back(on_cpu: bool, error: &str) -> bool {
    !on_cpu && (error.contains(NON_FINITE) || error.starts_with("inference failed:"))
}

const NON_FINITE: &str = "non-finite values";

pub(crate) fn is_non_finite(error: &ApiError) -> bool {
    error.message.contains(NON_FINITE)
}

/// Runs `primary`; a non-finite output of a GPU session is run again by `cpu` (the CPU
/// result, or its error, is the answer). Any other outcome stands.
pub(crate) fn with_cpu_retry<T>(
    on_cpu: bool,
    primary: impl FnOnce() -> Result<T, ApiError>,
    cpu: impl FnOnce() -> Result<T, ApiError>,
) -> Result<T, ApiError> {
    match primary() {
        Err(e) if !on_cpu && is_non_finite(&e) => cpu(),
        other => other,
    }
}

/// A CPU session of the same graph, created on the first request a GPU session answers with
/// non-finite values and kept for the later ones.
pub(crate) struct CpuFallback {
    model: PathBuf,
    threads: Option<usize>,
    session: OnceLock<SessionLock>,
    build_lock: Mutex<()>,
    /// Set once the warm-up passed: a non-finite warm-up reloads the engine on the CPU instead.
    armed: AtomicBool,
}

impl CpuFallback {
    pub(crate) fn new(model: PathBuf, threads: Option<usize>) -> Self {
        Self {
            model,
            threads,
            session: OnceLock::new(),
            build_lock: Mutex::new(()),
            armed: AtomicBool::new(false),
        }
    }

    pub(crate) fn arm(&self) {
        self.armed.store(true, Ordering::Release);
    }

    /// No CPU retry: the session is the CPU one, or the warm-up is still running.
    pub(crate) fn skip(&self, provider: &str) -> bool {
        provider == "CPU" || !self.armed.load(Ordering::Acquire)
    }

    pub(crate) fn get(&self) -> Result<&SessionLock, ApiError> {
        try_cached(&self.session, &self.build_lock, || {
            eprintln!(
                "harness-onnx: the GPU produced non-finite values for {}; retrying on the CPU",
                self.model.display()
            );
            build_session_with(&self.model, Provider::Cpu, self.threads, false)
                .map(|(session, _)| Mutex::new(session))
        })
        .map_err(|e| {
            ApiError::server(format!(
                "output contains {NON_FINITE} and the CPU retry failed: {e}"
            ))
        })
    }
}

// Serialize initialization, retaining only successful builds so transient failures can retry.
fn try_cached<'a, T>(
    cache: &'a OnceLock<T>,
    lock: &Mutex<()>,
    build: impl FnOnce() -> Result<T, String>,
) -> Result<&'a T, String> {
    if let Some(value) = cache.get() {
        return Ok(value);
    }
    let _guard = lock.lock().map_err(|e| e.to_string())?;
    if let Some(value) = cache.get() {
        return Ok(value);
    }
    let value = build()?;
    Ok(cache.get_or_init(|| value))
}

pub(crate) fn finish_profile(session: &SessionLock) {
    if std::env::var_os("HARNESS_ONNX_PROFILE").is_some() {
        if let Ok(mut session) = session.lock() {
            match session.end_profiling() {
                Ok(path) => println!("harness-onnx: warm-up profile: {path}"),
                Err(e) => eprintln!("harness-onnx: cannot finish warm-up profile: {e}"),
            }
        }
    }
}

pub(crate) fn finite_output(name: &str, data: &[f32]) -> Result<(), ApiError> {
    if data.iter().all(|v| v.is_finite()) {
        Ok(())
    } else {
        Err(ApiError::server(format!(
            "output {name} contains {NON_FINITE}"
        )))
    }
}

/// Loads the model: package files, tokenizer, ONNX session. Runs one warm-up inference so a
/// graph that loads but cannot run fails here, not on the first request. A GPU provider whose
/// warm-up fails or yields non-finite values falls back to the CPU.
pub fn load(opts: &LoadOptions) -> Result<Engine, String> {
    match load_once(opts, true) {
        Err(e) if warmup_falls_back(opts.provider == Provider::Cpu, &e) => {
            eprintln!("harness-onnx: warm-up failed: {e} (falling back to the CPU)");
            let mut cpu = opts.clone();
            cpu.provider = Provider::Cpu;
            load_once(&cpu, false)
        }
        result => result,
    }
}

fn load_once(opts: &LoadOptions, profile: bool) -> Result<Engine, String> {
    if opts.task == Some(Task::AsrCtc) {
        return Err("CTC models use asr_ctc::load".into());
    }
    let mut package = ModelPackage::discover(&opts.model).map_err(|e| e.to_string())?;
    if package.task == Task::AsrCtc {
        return Err("CTC models use asr_ctc::load".into());
    }
    if let Some(t) = opts.task {
        package.task = t;
    }
    if let Some(p) = opts.pooling {
        package.pooling = p;
    }
    if let Some(n) = opts.normalize {
        package.normalize = n;
    }
    if let Some(n) = opts.max_length {
        package.max_length = n.clamp(8, crate::package::MAX_SEQUENCE_LENGTH);
    }
    if !package.unsupported_modules.is_empty() && package.task == Task::Embedding {
        return Err(format!(
            "{} uses sentence-transformers modules this server cannot run ({}); its vectors would be wrong",
            package.name,
            package.unsupported_modules.join(", ")
        ));
    }

    let mut tokenizer = crate::text_tokenizer::load(&package.tokenizer_path)?;
    let pad_id = tokenizer
        .get_padding()
        .map(|p| p.pad_id)
        .or_else(|| {
            ["<pad>", "[PAD]", "<|endoftext|>"]
                .iter()
                .find_map(|t| tokenizer.token_to_id(t))
        })
        .unwrap_or(0);
    tokenizer.with_padding(None);
    tokenizer
        .with_truncation(Some(TruncationParams {
            direction: TruncationDirection::Right,
            max_length: package.max_length,
            strategy: TruncationStrategy::LongestFirst,
            stride: 0,
        }))
        .map_err(|e| format!("tokenizer truncation: {}", e))?;

    let qa_tokenizer = if package.task == Task::QuestionAnswering {
        let mut t = tokenizer.clone();
        t.with_truncation(None)
            .map_err(|e| format!("tokenizer truncation: {}", e))?;
        Some(t)
    } else {
        None
    };

    let (session, provider) =
        build_session_with(&package.onnx_path, opts.provider, opts.threads, profile)?;
    let inputs = classify_inputs(&session)?;
    let output = pick_output(&session, package.task)?;
    let engine = Engine {
        cpu: CpuFallback::new(package.onnx_path.clone(), opts.threads),
        session: Mutex::new(session),
        tokenizer,
        qa_tokenizer,
        package,
        inputs,
        output,
        pad_id,
        provider: provider.to_string(),
        batch_size: opts.batch_size.max(1),
        token_budget: 16384,
    };
    // Warm-up: also settles the output kind (vector or per-token) before the first request.
    let warm = (|| -> Result<(), String> {
        match engine.package.task {
            Task::AsrCtc => return Err("CTC models use asr_ctc::load".into()),
            Task::Embedding => {
                engine
                    .embed(&["warm up".to_string()], None)
                    .map_err(|e| e.message)?;
            }
            Task::Rerank => {
                engine
                    .rerank("warm up", &["warm up".to_string()])
                    .map_err(|e| e.message)?;
            }
            Task::Classify => {
                engine
                    .classify(&["warm up".to_string()], None, Some(1))
                    .map_err(|e| e.message)?;
            }
            Task::TokenClassify => {
                engine
                    .token_classify(&["warm up".to_string()], true)
                    .map_err(|e| e.message)?;
            }
            Task::FillMask => {
                let mask = engine.mask_token().ok_or_else(|| {
                    format!("{} has no mask token in its tokenizer", engine.package.name)
                })?;
                engine
                    .fill_mask(&format!("warm {mask}"), 1)
                    .map_err(|e| e.message)?;
            }
            Task::QuestionAnswering => {
                engine
                    .answer("what?", "warm up", 1)
                    .map_err(|e| e.message)?;
            }
        }
        Ok(())
    })();
    if profile {
        finish_profile(&engine.session);
    }
    warm?;
    engine.cpu.arm();
    Ok(engine)
}

/// One text (or pair) turned into model inputs.
struct Encoded {
    ids: Vec<u32>,
    type_ids: Vec<u32>,
    /// Byte offsets of each token in its own text (the first or the second of a pair).
    offsets: Vec<(usize, usize)>,
    special: Vec<bool>,
    words: Vec<Option<u32>>,
    /// Which text of a pair a token comes from (`None` for special tokens).
    sequence: Vec<Option<usize>>,
}

/// Raw output of one graph run.
enum Raw {
    /// `[batch, seq, dim]`
    Tokens {
        data: Vec<f32>,
        seq: usize,
        dim: usize,
    },
    /// `[batch, dim]`
    Rows { data: Vec<f32>, dim: usize },
}

fn to_f32(values: &[half::f16]) -> Vec<f32> {
    values.iter().map(|v| v.to_f32()).collect()
}

/// An output tensor: shape and f32 values.
type RawOutput = (Vec<i64>, Vec<f32>);

fn encoded_of(e: &tokenizers::Encoding) -> Encoded {
    Encoded {
        ids: e.get_ids().to_vec(),
        type_ids: e.get_type_ids().to_vec(),
        offsets: e.get_offsets().to_vec(),
        special: e
            .get_special_tokens_mask()
            .iter()
            .map(|&m| m != 0)
            .collect(),
        words: e.get_word_ids().to_vec(),
        sequence: e.get_sequence_ids(),
    }
}

/// A byte-level post-processor trims the offsets of a context that was already trimmed when
/// it was encoded alone (the leading space of a word token is dropped twice, so "London"
/// answers as "ondon"): the context tokens take the offsets of their stand-alone encoding back.
fn restore_context_offsets(mut e: Encoded, own: &[(usize, usize)]) -> Encoded {
    let slots: Vec<usize> = (0..e.ids.len())
        .filter(|&t| e.sequence[t] == Some(1))
        .collect();
    if slots.len() == own.len() {
        for (slot, offsets) in slots.into_iter().zip(own) {
            e.offsets[slot] = *offsets;
        }
    }
    e
}

/// The windows of [`Engine::qa_windows`] for any tokenizer without truncation.
fn qa_windows_of(
    tokenizer: &Tokenizer,
    max_length: usize,
    question: &str,
    context: &str,
) -> Result<Vec<Encoded>, ApiError> {
    let err = |e: tokenizers::Error| ApiError::invalid(format!("tokenization failed: {}", e));
    let q = tokenizer.encode(question, false).map_err(err)?;
    let mut c = tokenizer.encode(context, false).map_err(err)?;
    let specials = tokenizer
        .post_process(Default::default(), Some(Default::default()), true)
        .map_err(err)?
        .len();
    let room = max_length.saturating_sub(q.len() + specials);
    if room < 16 {
        return Err(ApiError::invalid(format!(
            "the question is too long: it leaves {} of the model's {} tokens for the context",
            room, max_length
        )));
    }
    let stride = 128.min(room / 2);
    c.truncate(room, stride, TruncationDirection::Right);
    let rest = c.take_overflowing();
    std::iter::once(c)
        .chain(rest)
        .map(|chunk| {
            let own = chunk.get_offsets().to_vec();
            tokenizer
                .post_process(q.clone(), Some(chunk), true)
                .map(|e| restore_context_offsets(encoded_of(&e), &own))
                .map_err(err)
        })
        .collect()
}

/// `(token, start, end)` of the context tokens of a window (no special tokens).
fn context_tokens_of(e: &Encoded) -> Vec<(usize, usize, usize)> {
    (0..e.ids.len())
        .filter(|&t| e.sequence[t] == Some(1) && !e.special[t])
        .map(|t| (t, e.offsets[t].0, e.offsets[t].1))
        .collect()
}

impl Engine {
    fn encode(&self, inputs: Vec<EncodeInput<'_>>) -> Result<Vec<Encoded>, ApiError> {
        let encodings = self
            .tokenizer
            .encode_batch(inputs, true)
            .map_err(|e| ApiError::invalid(format!("tokenization failed: {}", e)))?;
        Ok(encodings.iter().map(encoded_of).collect())
    }

    /// The tokenizer's mask token (`[MASK]`, `<mask>`).
    pub fn mask_token(&self) -> Option<String> {
        ["[MASK]", "<mask>", "<MASK>"]
            .iter()
            .find(|t| self.tokenizer.token_to_id(t).is_some())
            .map(|t| t.to_string())
    }

    fn require(&self, task: Task, what: &str) -> Result<(), ApiError> {
        if self.package.task == task {
            Ok(())
        } else {
            Err(ApiError::unavailable(format!(
                "this model is a {} model, not a {} model",
                self.package.task.as_str(),
                what
            )))
        }
    }

    /// Classifier logits of single texts or `(text, text_pair)` encodings, one row each.
    fn class_rows(&self, inputs: Vec<EncodeInput<'_>>) -> Result<(Vec<Vec<f32>>, usize), ApiError> {
        let encoded = self.encode(inputs)?;
        let lengths: Vec<usize> = encoded.iter().map(|e| e.ids.len()).collect();
        let tokens = lengths.iter().sum();
        let mut rows = vec![Vec::new(); encoded.len()];
        for group in self.plan(&lengths) {
            let batch: Vec<&Encoded> = group.iter().map(|&i| &encoded[i]).collect();
            match self.run(&batch)? {
                Raw::Rows { data, dim } => {
                    for (slot, row) in group.iter().zip(data.chunks(dim)) {
                        rows[*slot] = row.to_vec();
                    }
                }
                Raw::Tokens { .. } => {
                    return Err(ApiError::server(
                        "the graph outputs token vectors, not classifier logits",
                    ))
                }
            }
        }
        Ok((rows, tokens))
    }

    /// Label scores of each text (with its pair text, for a pair classifier), best first.
    pub fn classify(
        &self,
        texts: &[String],
        pairs: Option<&[String]>,
        top_k: Option<usize>,
    ) -> Result<(Vec<Vec<LabelScore>>, usize), ApiError> {
        if !matches!(self.package.task, Task::Classify | Task::Rerank) {
            return Err(ApiError::unavailable(format!(
                "this model is a {} model, not a text classifier",
                self.package.task.as_str()
            )));
        }
        let inputs = match pairs {
            Some(p) => texts
                .iter()
                .zip(p)
                .map(|(a, b)| EncodeInput::from((a.as_str(), b.as_str())))
                .collect(),
            None => texts
                .iter()
                .map(|t| EncodeInput::from(t.as_str()))
                .collect(),
        };
        let (rows, tokens) = self.class_rows(inputs)?;
        Ok((
            rows.iter()
                .map(|r| {
                    tasks::classify_row(r, &self.package.labels, self.package.multi_label, top_k)
                })
                .collect(),
            tokens,
        ))
    }

    /// Zero-shot scores of `candidates` for `text` with an NLI head: each candidate fills
    /// `template` (`{}` is the label) as the hypothesis.
    pub fn zero_shot(
        &self,
        text: &str,
        candidates: &[String],
        template: &str,
        multi_label: bool,
    ) -> Result<(Vec<LabelScore>, usize), ApiError> {
        self.require(Task::Classify, "zero-shot (NLI)")?;
        let (entail, contra) = tasks::nli_indices(&self.package.labels).ok_or_else(|| {
            ApiError::unavailable(
                "this classifier has no entailment label: zero-shot classification needs an NLI model",
            )
        })?;
        let hypotheses: Vec<String> = candidates
            .iter()
            .map(|c| {
                if template.contains("{}") {
                    template.replace("{}", c)
                } else {
                    format!("{template} {c}")
                }
            })
            .collect();
        let inputs = hypotheses
            .iter()
            .map(|h| EncodeInput::from((text, h.as_str())))
            .collect();
        let (rows, tokens) = self.class_rows(inputs)?;
        Ok((
            tasks::zero_shot(&rows, candidates, entail, contra, multi_label),
            tokens,
        ))
    }

    /// Entities of each text (`group`: tokens merged into words and entities).
    pub fn token_classify(
        &self,
        texts: &[String],
        group: bool,
    ) -> Result<(Vec<Vec<Entity>>, usize), ApiError> {
        self.require(Task::TokenClassify, "token-classification")?;
        let encoded = self.encode(
            texts
                .iter()
                .map(|t| EncodeInput::from(t.as_str()))
                .collect(),
        )?;
        let lengths: Vec<usize> = encoded.iter().map(|e| e.ids.len()).collect();
        let tokens = lengths.iter().sum();
        let mut out = vec![Vec::new(); texts.len()];
        for indices in self.plan(&lengths) {
            let batch: Vec<&Encoded> = indices.iter().map(|&i| &encoded[i]).collect();
            let Raw::Tokens { data, seq, dim } = self.run(&batch)? else {
                return Err(ApiError::server(
                    "the graph does not output per-token logits",
                ));
            };
            for (b, &slot) in indices.iter().enumerate() {
                let e = &encoded[slot];
                let info: Vec<TokenInfo> = (0..e.ids.len())
                    .map(|t| TokenInfo {
                        start: e.offsets[t].0,
                        end: e.offsets[t].1,
                        special: e.special[t],
                        word: e.words[t],
                    })
                    .collect();
                let rows = &data[b * seq * dim..(b + 1) * seq * dim];
                out[slot] = tasks::token_entities(
                    &texts[slot],
                    rows,
                    dim,
                    &info,
                    &self.package.labels,
                    group,
                );
            }
        }
        Ok((out, tokens))
    }

    /// The `top_k` fills of the first mask token of `text`: token, id, score, filled text.
    #[allow(clippy::type_complexity)]
    pub fn fill_mask(
        &self,
        text: &str,
        top_k: usize,
    ) -> Result<(Vec<(String, u32, f32, String)>, usize), ApiError> {
        self.require(Task::FillMask, "fill-mask")?;
        let mask = self
            .mask_token()
            .ok_or_else(|| ApiError::server("the tokenizer has no mask token"))?;
        let mask_id = self.tokenizer.token_to_id(&mask).unwrap_or(u32::MAX);
        let encoded = self.encode(vec![EncodeInput::from(text)])?;
        let e = &encoded[0];
        let position = e.ids.iter().position(|&id| id == mask_id).ok_or_else(|| {
            ApiError::invalid(if text.contains(&mask) {
                format!(
                    "the input is longer than the model's {} tokens and was cut before {mask}: shorten the text before it",
                    self.package.max_length
                )
            } else {
                format!("the input has no mask token: put {mask} where the word goes")
            })
        })?;
        let Raw::Tokens { data, dim, .. } = self.run(&[e])? else {
            return Err(ApiError::server(
                "the graph does not output per-token logits",
            ));
        };
        let row = &data[position * dim..(position + 1) * dim];
        let fills = tasks::top_tokens(row, top_k)
            .into_iter()
            .map(|(id, score)| {
                let token = self
                    .tokenizer
                    .decode(&[id], true)
                    .unwrap_or_default()
                    .trim()
                    .to_string();
                let sequence = text.replacen(&mask, &token, 1);
                (token, id, score, sequence)
            })
            .collect();
        Ok((fills, e.ids.len()))
    }

    /// `question` with each window of `context`: windows as long as the model allows next to
    /// the question and the special tokens, overlapping by the pipeline's `doc_stride` (128
    /// tokens, less for a short room).
    fn qa_windows(&self, question: &str, context: &str) -> Result<Vec<Encoded>, ApiError> {
        let tokenizer = self.qa_tokenizer.as_ref().unwrap_or(&self.tokenizer);
        qa_windows_of(tokenizer, self.package.max_length, question, context)
    }

    /// The best `top_k` answers to `question` in `context` (an extractive QA head).
    pub fn answer(
        &self,
        question: &str,
        context: &str,
        top_k: usize,
    ) -> Result<(Vec<Answer>, usize), ApiError> {
        self.require(Task::QuestionAnswering, "question-answering")?;
        let windows = self.qa_windows(question, context)?;
        if windows.len() > MAX_QA_WINDOWS {
            return Err(ApiError::invalid(format!(
                "the context needs {} windows of {} tokens; at most {} per request",
                windows.len(),
                self.package.max_length,
                MAX_QA_WINDOWS
            )));
        }
        let mut answers: Vec<Answer> = Vec::new();
        let mut tokens = 0;
        for e in &windows {
            tokens += e.ids.len();
            let outputs = self.run_outputs(&[e])?;
            let [start, end] = outputs.as_slice() else {
                return Err(ApiError::server("the graph returned no start/end logits"));
            };
            let context_tokens = context_tokens_of(e);
            // `[CLS]` (the first token when it is a special one) is the "no answer" choice.
            let null = e.special.first().copied().unwrap_or(false).then_some(0);
            answers.extend(tasks::answer_spans(
                context,
                start,
                end,
                &context_tokens,
                null,
                30,
                top_k,
            ));
        }
        // The best spans over all windows; a span found in two overlapping windows once.
        answers.sort_by(|a, b| b.score.total_cmp(&a.score));
        let mut out: Vec<Answer> = Vec::new();
        for a in answers {
            if !out.iter().any(|o| o.start == a.start && o.end == a.end) {
                out.push(a);
            }
        }
        out.truncate(top_k.max(1));
        Ok((out, tokens))
    }

    /// The flat values of the spec's outputs (`name`, then `extra`) for `batch`.
    fn run_outputs(&self, batch: &[&Encoded]) -> Result<Vec<Vec<f32>>, ApiError> {
        Ok(self
            .run_raw(batch, true)?
            .into_iter()
            .map(|(_, d)| d)
            .collect())
    }

    /// Runs the graph on `batch` (right-padded to the longest). Returns the raw output.
    fn run(&self, batch: &[&Encoded]) -> Result<Raw, ApiError> {
        let n = batch.len();
        let (shape, data) = self.run_raw(batch, false)?.remove(0);
        match shape.as_slice() {
            [b, s, d] if *b as usize == n => Ok(Raw::Tokens {
                data,
                seq: *s as usize,
                dim: *d as usize,
            }),
            [b, d] if *b as usize == n => Ok(Raw::Rows {
                data,
                dim: *d as usize,
            }),
            [b] if *b as usize == n => Ok(Raw::Rows { data, dim: 1 }),
            other => Err(ApiError::server(format!(
                "unexpected output shape {:?} for a batch of {}",
                other, n
            ))),
        }
    }

    /// Feeds `batch` (right-padded to the longest) and reads the spec's output, and its
    /// `extra` one when `all`, as shape and f32 values.
    fn run_raw(&self, batch: &[&Encoded], all: bool) -> Result<Vec<RawOutput>, ApiError> {
        let n = batch.len();
        let seq = batch.iter().map(|e| e.ids.len()).max().unwrap_or(1).max(1);
        let shape = [n, seq];
        let fill = |value: &dyn Fn(&Encoded, usize) -> i64| -> Vec<i64> {
            let mut out = Vec::with_capacity(n * seq);
            for e in batch {
                for t in 0..seq {
                    out.push(value(e, t));
                }
            }
            out
        };
        let ids = fill(&|e, t| e.ids.get(t).map_or(self.pad_id as i64, |&v| v as i64));
        let mask = fill(&|e, t| (t < e.ids.len()) as i64);
        let type_ids = fill(&|e, t| e.type_ids.get(t).map_or(0, |&v| v as i64));
        let positions = fill(&|e, t| t.min(e.ids.len().saturating_sub(1)) as i64);

        let tensor = |data: Vec<i64>, ty: IdType| -> Result<ort::value::DynTensor, ApiError> {
            let err = |e: ort::Error| ApiError::server(format!("tensor: {}", e));
            Ok(match ty {
                IdType::Int64 => Tensor::from_array((shape, data)).map_err(err)?.upcast(),
                IdType::Int32 => Tensor::from_array((
                    shape,
                    data.into_iter().map(|v| v as i32).collect::<Vec<i32>>(),
                ))
                .map_err(err)?
                .upcast(),
            })
        };
        let feeds = || -> Result<Vec<(String, ort::value::DynTensor)>, ApiError> {
            let mut feeds: Vec<(String, ort::value::DynTensor)> = vec![(
                self.inputs.ids.0.clone(),
                tensor(ids.clone(), self.inputs.ids.1)?,
            )];
            if let Some((name, ty)) = &self.inputs.mask {
                feeds.push((name.clone(), tensor(mask.clone(), *ty)?));
            }
            if let Some((name, ty)) = &self.inputs.type_ids {
                feeds.push((name.clone(), tensor(type_ids.clone(), *ty)?));
            }
            if let Some((name, ty)) = &self.inputs.position_ids {
                feeds.push((name.clone(), tensor(positions.clone(), *ty)?));
            }
            Ok(feeds)
        };
        let mut names = vec![self.output.name.clone()];
        if all {
            names.extend(self.output.extra.clone());
        }
        with_cpu_retry(
            self.cpu.skip(&self.provider),
            || self.exec(&self.session, feeds()?, &names),
            || self.exec(self.cpu.get()?, feeds()?, &names),
        )
    }

    /// Runs `feeds` on `session` and reads the `names` outputs; non-finite values are an error.
    fn exec(
        &self,
        session: &SessionLock,
        feeds: Vec<(String, ort::value::DynTensor)>,
        names: &[String],
    ) -> Result<Vec<RawOutput>, ApiError> {
        let mut session = session
            .lock()
            .map_err(|_| ApiError::server("the model session is poisoned"))?;
        let outputs = session
            .run(feeds)
            .map_err(|e| ApiError::server(format!("inference failed: {}", e)))?;
        let mut out = Vec::with_capacity(names.len());
        for name in names {
            let value = outputs
                .get(name)
                .ok_or_else(|| ApiError::server(format!("output {} missing", name)))?;
            let err = |e: ort::Error| ApiError::server(format!("output: {}", e));
            out.push(match value.dtype().tensor_type() {
                Some(TensorElementType::Float16) => {
                    let (s, d) = value.try_extract_tensor::<half::f16>().map_err(err)?;
                    (s.to_vec(), to_f32(d))
                }
                _ => {
                    let (s, d) = value.try_extract_tensor::<f32>().map_err(err)?;
                    (s.to_vec(), d.to_vec())
                }
            });
            finite_output(name, &out.last().expect("output just added").1)?;
        }
        Ok(out)
    }

    /// Groups texts into graph runs: sorted by length (little padding), at most
    /// `batch_size` texts and `token_budget` padded tokens each. Returns index groups.
    fn plan(&self, lengths: &[usize]) -> Vec<Vec<usize>> {
        let mut order: Vec<usize> = (0..lengths.len()).collect();
        order.sort_by_key(|&i| lengths[i]);
        let mut groups: Vec<Vec<usize>> = Vec::new();
        let mut current: Vec<usize> = Vec::new();
        for i in order {
            let longest = lengths[i].max(1);
            if !current.is_empty()
                && (current.len() >= self.batch_size
                    || (current.len() + 1) * longest > self.token_budget)
            {
                groups.push(std::mem::take(&mut current));
            }
            current.push(i);
        }
        if !current.is_empty() {
            groups.push(current);
        }
        groups
    }

    /// Embeddings of `texts` and the number of tokens read.
    pub fn embed(
        &self,
        texts: &[String],
        dimensions: Option<usize>,
    ) -> Result<(Vec<Vec<f32>>, usize), ApiError> {
        if self.package.task != Task::Embedding {
            return Err(ApiError::unavailable(
                "this model is a reranker: use /v1/rerank (embeddings need an embedding model)",
            ));
        }
        let encoded = self.encode(
            texts
                .iter()
                .map(|t| EncodeInput::from(t.as_str()))
                .collect(),
        )?;
        let lengths: Vec<usize> = encoded.iter().map(|e| e.ids.len()).collect();
        let tokens = lengths.iter().sum();
        let mut out: Vec<Vec<f32>> = vec![Vec::new(); texts.len()];
        for group in self.plan(&lengths) {
            let batch: Vec<&Encoded> = group.iter().map(|&i| &encoded[i]).collect();
            let lens: Vec<usize> = batch.iter().map(|e| e.ids.len()).collect();
            let vectors = match self.run(&batch)? {
                Raw::Tokens { data, seq, dim } => {
                    pooling::pool(self.package.pooling, &data, batch.len(), seq, dim, &lens)
                }
                Raw::Rows { data, dim } => data.chunks(dim).map(<[f32]>::to_vec).collect(),
            };
            for (slot, mut v) in group.iter().zip(vectors) {
                if self.package.normalize {
                    pooling::l2_normalize(&mut v);
                }
                if let Some(d) = dimensions {
                    pooling::truncate_dims(&mut v, d, self.package.normalize);
                }
                out[*slot] = v;
            }
        }
        Ok((out, tokens))
    }

    /// Relevance logits of `documents` for `query` (input order) and tokens read.
    pub fn rerank(&self, query: &str, documents: &[String]) -> Result<(Vec<f32>, usize), ApiError> {
        if self.package.task != Task::Rerank {
            return Err(ApiError::unavailable(
                "this model is an embedding model: use /v1/embeddings (reranking needs a cross-encoder)",
            ));
        }
        let encoded = self.encode(
            documents
                .iter()
                .map(|d| EncodeInput::from((query, d.as_str())))
                .collect(),
        )?;
        let lengths: Vec<usize> = encoded.iter().map(|e| e.ids.len()).collect();
        let tokens = lengths.iter().sum();
        let mut scores = vec![0f32; documents.len()];
        for group in self.plan(&lengths) {
            let batch: Vec<&Encoded> = group.iter().map(|&i| &encoded[i]).collect();
            match self.run(&batch)? {
                Raw::Rows { data, dim } => {
                    for (slot, row) in group.iter().zip(data.chunks(dim)) {
                        scores[*slot] = pooling::rerank_score(row);
                    }
                }
                Raw::Tokens { .. } => return Err(ApiError::server(
                    "the graph outputs token vectors, not classifier logits: not a cross-encoder",
                )),
            }
        }
        Ok((scores, tokens))
    }

    /// One line for the startup log.
    pub fn describe(&self) -> String {
        format!(
            "{} ({}), {}, max {} tokens, output {}",
            self.package.name,
            self.package.task.as_str(),
            self.provider,
            self.package.max_length,
            self.output.name
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn non_finite_outputs_are_errors_before_postprocessing() {
        assert!(finite_output("logits", &[0.0, -1.0, f32::MAX]).is_ok());
        for value in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY] {
            let error = finite_output("pred_boxes", &[0.5, value]).unwrap_err();
            assert!(error.message.contains("pred_boxes"));
            assert!(error.message.contains("non-finite"));
        }
    }

    fn finite_err() -> ApiError {
        finite_output("logits", &[f32::NAN]).unwrap_err()
    }

    #[test]
    fn a_failed_gpu_warm_up_falls_back_to_the_cpu() {
        let nan = finite_err().message;
        assert!(warmup_falls_back(false, &nan));
        assert!(warmup_falls_back(false, "inference failed: device lost"));
        // Anything else (a missing tokenizer, a bad graph) is the same on the CPU.
        assert!(!warmup_falls_back(false, "cannot read tokenizer.json"));
        // The CPU is the last stop.
        assert!(!warmup_falls_back(true, &nan));
        assert!(!warmup_falls_back(true, "inference failed: out of memory"));
    }

    #[test]
    fn the_cpu_retry_of_a_failed_warm_up_is_not_profiled() {
        let path = Some(std::ffi::OsString::from("profile"));
        assert_eq!(profile_target(path.clone(), true), path);
        assert_eq!(profile_target(path, false), None);
        assert_eq!(profile_target(None, true), None);
    }

    #[test]
    fn a_non_finite_gpu_answer_is_retried_on_the_cpu_once() {
        let cpu_calls = std::cell::Cell::new(0);
        let cpu = || {
            cpu_calls.set(cpu_calls.get() + 1);
            Ok(7)
        };
        // A finite GPU answer stands.
        assert_eq!(with_cpu_retry(false, || Ok(1), cpu).unwrap(), 1);
        // Another GPU failure is reported, not retried.
        let other = with_cpu_retry(false, || Err(ApiError::server("inference failed: x")), cpu);
        assert!(other.unwrap_err().message.contains("inference failed"));
        assert_eq!(cpu_calls.get(), 0);
        // Non-finite values on the GPU: the CPU answer is the answer.
        assert_eq!(with_cpu_retry(false, || Err(finite_err()), cpu).unwrap(), 7);
        assert_eq!(cpu_calls.get(), 1);
        // The CPU failing too is an error.
        let failed = with_cpu_retry(
            false,
            || Err(finite_err()),
            || Err::<i32, _>(ApiError::server("cpu failed")),
        );
        assert_eq!(failed.unwrap_err().message, "cpu failed");
        // A CPU session (or a session still warming up) has nothing to retry on.
        let on_cpu = with_cpu_retry(true, || Err(finite_err()), cpu);
        assert!(is_non_finite(&on_cpu.unwrap_err()));
        assert_eq!(cpu_calls.get(), 1);
    }

    #[test]
    fn the_cpu_retry_is_armed_after_the_warm_up_only() {
        let fallback = CpuFallback::new(PathBuf::from("model.onnx"), None);
        assert!(fallback.skip("DirectML"));
        fallback.arm();
        assert!(!fallback.skip("DirectML"));
        assert!(fallback.skip("CPU"));
    }

    /// A RoBERTa-style byte-level BPE tokenizer: every piece is a vocabulary entry built by
    /// merges from its characters; word pieces carry the `Ġ` space marker.
    fn byte_level_bpe(pieces: &[&str]) -> Tokenizer {
        let mut vocab: Vec<String> = ["<s>", "<pad>", "</s>", "<unk>"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        let mut merges = Vec::new();
        let add = |vocab: &mut Vec<String>, t: String| {
            if !vocab.contains(&t) {
                vocab.push(t);
            }
        };
        for piece in pieces {
            let chars: Vec<String> = piece.chars().map(String::from).collect();
            for c in &chars {
                add(&mut vocab, c.clone());
            }
            let mut built = chars[0].clone();
            for c in &chars[1..] {
                merges.push(format!("{built} {c}"));
                built.push_str(c);
                add(&mut vocab, built.clone());
            }
        }
        let vocab: serde_json::Map<String, serde_json::Value> = vocab
            .into_iter()
            .enumerate()
            .map(|(i, t)| (t, serde_json::json!(i)))
            .collect();
        let special = |id: u32, content: &str| {
            serde_json::json!({
                "id": id, "content": content, "single_word": false, "lstrip": false,
                "rstrip": false, "normalized": false, "special": true
            })
        };
        let json = serde_json::json!({
            "version": "1.0",
            "added_tokens": [
                special(0, "<s>"), special(1, "<pad>"), special(2, "</s>"), special(3, "<unk>")
            ],
            "normalizer": null,
            "pre_tokenizer": {
                "type": "ByteLevel", "add_prefix_space": false, "trim_offsets": true,
                "use_regex": true
            },
            "post_processor": {
                "type": "RobertaProcessing", "sep": ["</s>", 2], "cls": ["<s>", 0],
                "trim_offsets": true, "add_prefix_space": false
            },
            "decoder": {
                "type": "ByteLevel", "add_prefix_space": true, "trim_offsets": true,
                "use_regex": true
            },
            "model": {
                "type": "BPE", "dropout": null, "unk_token": null,
                "continuing_subword_prefix": "", "end_of_word_suffix": "", "fuse_unk": false,
                "byte_fallback": false, "ignore_merges": false,
                "vocab": vocab, "merges": merges
            }
        });
        json.to_string().parse().expect("a valid tokenizer.json")
    }

    /// The best span of every window when the model puts all its weight on the token `pick`
    /// chooses (start and end).
    fn answer_at(
        windows: &[Encoded],
        context: &str,
        pick: impl Fn(&Encoded) -> Option<usize>,
    ) -> Vec<String> {
        windows
            .iter()
            .filter_map(|e| {
                let t = pick(e)?;
                let mut logits = vec![-10.0f32; e.ids.len()];
                logits[t] = 10.0;
                let spans = tasks::answer_spans(
                    context,
                    &logits,
                    &logits,
                    &context_tokens_of(e),
                    e.special.first().copied().unwrap_or(false).then_some(0),
                    30,
                    1,
                );
                spans.first().map(|a| a.answer.clone())
            })
            .collect()
    }

    /// The first context token with this id.
    fn context_token(id: u32) -> impl Fn(&Encoded) -> Option<usize> {
        move |e| (0..e.ids.len()).find(|&t| e.ids[t] == id && e.sequence[t] == Some(1))
    }

    #[test]
    fn roberta_byte_level_qa_returns_the_exact_substring() {
        let tokenizer = byte_level_bpe(&[
            "Where", "Ġdoes", "Ġhe", "Ġlive", "?", "He", "Ġlives", "Ġin", "ĠLondon", ".",
        ]);
        let context = "He lives in London.";
        let windows = qa_windows_of(&tokenizer, 64, "Where does he live?", context).unwrap();
        assert_eq!(windows.len(), 1);
        // "London" and not "ondon": the space marker is not part of the answer, and the first
        // letter is not lost to a second trim.
        let london = tokenizer.token_to_id("ĠLondon").unwrap();
        assert_eq!(
            answer_at(&windows, context, context_token(london)),
            ["London"]
        );
        let lives = tokenizer.token_to_id("Ġlives").unwrap();
        assert_eq!(
            answer_at(&windows, context, context_token(lives)),
            ["lives"]
        );
        // The sentence-initial word has no marker.
        let he = tokenizer.token_to_id("He").unwrap();
        assert_eq!(answer_at(&windows, context, context_token(he)), ["He"]);
    }

    #[test]
    fn bert_wordpiece_qa_returns_the_exact_substring() {
        let dir = tempfile::tempdir().unwrap();
        let vocab = dir.path().join("vocab.txt");
        std::fs::write(
            &vocab,
            "[PAD]\n[UNK]\n[CLS]\n[SEP]\n[MASK]\nwhere\ndoes\nhe\nlive\n?\nlives\nin\nlondon\n##s\n,\n.\nuk\n",
        )
        .unwrap();
        let tokenizer = crate::text_tokenizer::load(&vocab).unwrap();
        let context = "He lives in London, UK.";
        let windows = qa_windows_of(&tokenizer, 64, "Where does he live?", context).unwrap();
        assert_eq!(windows.len(), 1);
        // Lowercased tokens map back to the original casing of the context.
        let london = tokenizer.token_to_id("london").unwrap();
        assert_eq!(
            answer_at(&windows, context, context_token(london)),
            ["London"]
        );
        let uk = tokenizer.token_to_id("uk").unwrap();
        assert_eq!(answer_at(&windows, context, context_token(uk)), ["UK"]);
        // The question is never part of an answer, and the special tokens are none.
        let e = &windows[0];
        assert!(context_tokens_of(e).iter().all(|(t, _, _)| !e.special[*t]));
        assert!(context_tokens_of(e)
            .iter()
            .all(|(t, _, _)| e.sequence[*t] == Some(1)));
    }

    #[test]
    fn context_offsets_come_from_the_stand_alone_encoding() {
        let e = Encoded {
            ids: vec![0, 1, 2, 3],
            type_ids: vec![0; 4],
            offsets: vec![(0, 0), (0, 5), (39, 44), (0, 0)],
            special: vec![true, false, false, true],
            words: vec![None; 4],
            sequence: vec![None, Some(0), Some(1), None],
        };
        let fixed = restore_context_offsets(e, &[(38, 44)]);
        assert_eq!(fixed.offsets[2], (38, 44));
        assert_eq!(fixed.offsets[1], (0, 5));
    }

    #[test]
    fn provider_names() {
        assert_eq!(Provider::parse("DML"), Some(Provider::DirectMl));
        assert_eq!(Provider::parse("auto"), Some(Provider::Auto));
        assert_eq!(Provider::parse("cuda"), Some(Provider::Cuda));
        assert_eq!(Provider::parse("tpu"), None);
    }

    #[test]
    fn id_types() {
        assert_eq!(id_type(Some(TensorElementType::Int64)), Some(IdType::Int64));
        assert_eq!(id_type(Some(TensorElementType::Float32)), None);
        assert_eq!(id_type(None), None);
    }
}

#[cfg(test)]
mod cpu_cache_tests {
    use super::*;
    #[test]
    fn failed_build_is_retried_and_success_is_reused() {
        let cache = OnceLock::new();
        let lock = Mutex::new(());
        assert!(try_cached::<u8>(&cache, &lock, || Err("transient".into())).is_err());
        assert_eq!(*try_cached(&cache, &lock, || Ok(7)).unwrap(), 7);
        assert_eq!(
            *try_cached(&cache, &lock, || panic!("already built")).unwrap(),
            7
        );
    }
}
