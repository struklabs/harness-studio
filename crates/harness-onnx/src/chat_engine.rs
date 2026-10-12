//! A chat model on onnxruntime-genai: prompt building, the generation loop, stop handling.
//!
//! One generation runs at a time (a mutex around the loop): a genai generator owns the KV cache
//! of its conversation, and DirectML and CPU sessions are not shared between concurrent runs
//! well. Requests queue; the gateway's scheduler decides which model gets the GPU.

use crate::api::ApiError;
use crate::chat::{ChatJob, ChatTemplate, Finish, StopFilter, Usage};
use crate::genai::{Genai, Model, SearchOptions};
use crate::genai_pkg::GenaiPackage;
use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::Instant;

/// Tokens of context a conversation may use when the launch does not say (`--ctx-size`).
pub const DEFAULT_CONTEXT: usize = 4096;

pub struct ChatEngine {
    model: Model,
    pub package: GenaiPackage,
    template: Option<ChatTemplate>,
    /// Largest `prompt + answer` served: the launch's context size, at most the model's.
    pub context: usize,
    /// `directml`, `cuda`, `cpu`, or what the model's own config picks.
    pub provider: String,
    running: Mutex<()>,
}

/// A request turned into tokens, ready to run.
pub struct Prepared {
    pub prompt: Vec<i32>,
    pub options: SearchOptions,
    /// Most tokens the answer may have.
    pub max_new: usize,
    pub stop: Vec<String>,
}

impl ChatEngine {
    /// Loads the model in `package.dir`. `provider` is genai's name (`cpu`, `dml`, `cuda`) or
    /// `None` to run on what `genai_config.json` asks for. `context` 0 means the default.
    pub fn load(
        lib: &Arc<Genai>,
        package: GenaiPackage,
        provider: Option<&str>,
        context: usize,
    ) -> Result<Self, String> {
        let model = match Model::load(lib, &package.dir, provider, &[]) {
            // genai captures the whole decode graph on DirectML. A model with an operator that
            // DirectML does not run (a generic export, not one of Microsoft's `directml-*`
            // folders) cannot be captured; it runs without.
            Err(e) if provider == Some("dml") && e.contains("graph capture") => Model::load(
                lib,
                &package.dir,
                provider,
                &[("enable_graph_capture", "0")],
            )?,
            other => other?,
        };
        let template = ChatTemplate::load(&package.dir);
        let cap = if package.context_length > 0 {
            package.context_length
        } else {
            usize::MAX
        };
        let wanted = if context == 0 {
            DEFAULT_CONTEXT
        } else {
            context
        };
        let shown = match provider {
            Some("dml") => "directml".to_string(),
            Some(other) => other.to_string(),
            None => package.config_provider().to_string(),
        };
        Ok(Self {
            model,
            template,
            context: wanted.min(cap).max(64),
            provider: shown,
            package,
            running: Mutex::new(()),
        })
    }

    pub fn describe(&self) -> String {
        format!(
            "{} ({}), provider {}, context {} tokens, chat template: {}",
            self.package
                .dir
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default(),
            self.package.model_type,
            self.provider,
            self.context,
            self.template
                .as_ref()
                .map(|t| t.origin.as_str())
                .unwrap_or("genai's own")
        )
    }

    /// The prompt text of `job`.
    fn prompt_text(&self, job: &ChatJob) -> Result<String, ApiError> {
        match &self.template {
            Some(t) => t.render(&job.messages, &job.template_kwargs),
            None => {
                let messages = serde_json::to_string(&job.messages)
                    .map_err(|e| ApiError::server(e.to_string()))?;
                self.model.apply_chat_template(&messages).map_err(|e| {
                    ApiError::invalid(format!(
                        "this model has no chat template and genai could not build one: {e}"
                    ))
                })
            }
        }
    }

    /// Renders and tokenizes a request and fits it into the context.
    pub fn prepare(&self, job: &ChatJob) -> Result<Prepared, ApiError> {
        let text = self.prompt_text(job)?;
        let prompt = self.model.encode(&text).map_err(ApiError::server)?;
        if prompt.is_empty() {
            return Err(ApiError::invalid("the prompt is empty"));
        }
        if prompt.len() >= self.context {
            return Err(ApiError::invalid(format!(
                "the prompt has {} tokens but the context holds {} (raise the context size of the model)",
                prompt.len(),
                self.context
            )));
        }
        let room = self.context - prompt.len();
        let max_new = job.max_tokens.map_or(room, |n| n.min(room));
        let pkg = &self.package;
        let options = SearchOptions {
            max_length: prompt.len() + max_new,
            temperature: job.temperature.or(pkg.temperature).unwrap_or(0.7),
            top_p: job.top_p.or(pkg.top_p).unwrap_or(0.95),
            top_k: job.top_k.or(pkg.top_k).unwrap_or(40),
            repetition_penalty: job.repetition_penalty.unwrap_or(1.0),
            seed: job.seed,
        };
        Ok(Prepared {
            prompt,
            options,
            max_new,
            stop: job.stop.clone(),
        })
    }

    /// Runs the generation, calling `emit` with each piece of text; `emit` returns `false` when
    /// the client is gone. Blocking.
    pub fn run(
        &self,
        prepared: &Prepared,
        mut emit: impl FnMut(&str) -> bool,
    ) -> Result<(Finish, Usage), ApiError> {
        let _one_at_a_time = self.running.lock().unwrap_or_else(|e| e.into_inner());
        let started = Instant::now();
        let mut generation = self
            .model
            .generate(&prepared.prompt, &prepared.options)
            .map_err(ApiError::server)?;
        let mut filter = StopFilter::new(prepared.stop.clone());
        let mut produced = 0usize;
        let mut first_token: Option<f64> = None;
        let mut client_gone = false;
        let mut at_start = true;
        while !generation.is_done() && produced < prepared.max_new {
            let (_token, text) = generation.next_token().map_err(ApiError::server)?;
            produced += 1;
            if first_token.is_none() {
                first_token = Some(started.elapsed().as_secs_f64());
            }
            let mut out = filter.push(&text);
            // SentencePiece tokenizers (Phi-3, Llama 2) open an answer with a space-marked
            // word; transformers' decoders drop that space, so does this.
            if at_start && !out.is_empty() {
                at_start = false;
                if let Some(rest) = out.strip_prefix(' ') {
                    out = rest.to_string();
                }
            }
            if !out.is_empty() && !emit(&out) {
                client_gone = true;
                break;
            }
            if filter.stopped {
                break;
            }
        }
        let tail = filter.finish();
        if !tail.is_empty() && !client_gone {
            emit(&tail);
        }
        let total = started.elapsed().as_secs_f64();
        let prompt_secs = first_token.unwrap_or(total);
        let usage = Usage {
            prompt_tokens: prepared.prompt.len(),
            completion_tokens: produced,
            prompt_secs,
            generation_secs: (total - prompt_secs).max(0.0),
        };
        // The model ended on its own (end-of-sequence token), a stop string matched, or the
        // answer used its whole allowance.
        let finish = if filter.stopped || (produced < prepared.max_new && generation.is_done()) {
            Finish::Stop
        } else {
            Finish::Length
        };
        Ok((finish, usage))
    }
}

/// The folder a `--model` argument names: a `genai_config.json`, the folder, or the decoder
/// `.onnx` inside it.
pub fn package_of(model: &Path) -> Option<GenaiPackage> {
    GenaiPackage::discover(model)
}
