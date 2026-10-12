//! onnxruntime-genai through its C API (`onnxruntime-genai.dll`, loaded at run time).
//!
//! No Rust binding crate exists for it that tracks the releases, and the C API is small and
//! stable, so this module binds the ~25 functions a chat server needs with `libloading` and
//! wraps them in RAII types. The library is the managed download of Harness (SHA-256 pinned,
//! see `harness-core::onnx_runtime`); it loads `onnxruntime.dll` by name, so [`Genai::load`]
//! first loads the managed ONNX Runtime (and `DirectML.dll`) from their full paths: a
//! `LoadLibrary("onnxruntime.dll")` that comes later returns the module that is already there,
//! not the (older) copy Windows keeps in System32.
//!
//! Every call that can fail returns an `OgaResult*`; [`check`] turns it into a `String` error.

use libloading::Library;
use std::ffi::{c_char, c_void, CStr, CString};
use std::path::Path;
use std::ptr;
use std::sync::Arc;

type OgaResult = c_void;
type Res = *mut OgaResult;

macro_rules! api {
    ($( $field:ident : $name:literal => fn( $($arg:ty),* ) $(-> $ret:ty)? ),+ $(,)?) => {
        /// The C functions in use.
        #[allow(non_snake_case)]
        struct Api {
            $( $field: unsafe extern "C" fn($($arg),*) $(-> $ret)?, )+
        }

        impl Api {
            fn load(lib: &Library) -> Result<Self, String> {
                Ok(Self {
                    $( $field: unsafe {
                        *lib.get::<unsafe extern "C" fn($($arg),*) $(-> $ret)?>(concat!($name, "\0").as_bytes())
                            .map_err(|e| format!("onnxruntime-genai does not export {}: {}", $name, e))?
                    }, )+
                })
            }
        }
    };
}

api! {
    result_error: "OgaResultGetError" => fn(*const OgaResult) -> *const c_char,
    destroy_result: "OgaDestroyResult" => fn(*mut OgaResult),
    destroy_string: "OgaDestroyString" => fn(*const c_char),
    create_config: "OgaCreateConfig" => fn(*const c_char, *mut *mut c_void) -> Res,
    config_clear_providers: "OgaConfigClearProviders" => fn(*mut c_void) -> Res,
    config_append_provider: "OgaConfigAppendProvider" => fn(*mut c_void, *const c_char) -> Res,
    config_set_provider_option: "OgaConfigSetProviderOption" => fn(*mut c_void, *const c_char, *const c_char, *const c_char) -> Res,
    destroy_config: "OgaDestroyConfig" => fn(*mut c_void),
    create_model_from_config: "OgaCreateModelFromConfig" => fn(*const c_void, *mut *mut c_void) -> Res,
    destroy_model: "OgaDestroyModel" => fn(*mut c_void),
    create_tokenizer: "OgaCreateTokenizer" => fn(*const c_void, *mut *mut c_void) -> Res,
    destroy_tokenizer: "OgaDestroyTokenizer" => fn(*mut c_void),
    create_sequences: "OgaCreateSequences" => fn(*mut *mut c_void) -> Res,
    destroy_sequences: "OgaDestroySequences" => fn(*mut c_void),
    sequences_count: "OgaSequencesGetSequenceCount" => fn(*const c_void, usize) -> usize,
    sequences_data: "OgaSequencesGetSequenceData" => fn(*const c_void, usize) -> *const i32,
    tokenizer_encode: "OgaTokenizerEncode" => fn(*const c_void, *const c_char, *mut c_void) -> Res,
    tokenizer_apply_template: "OgaTokenizerApplyChatTemplate" => fn(*const c_void, *const c_char, *const c_char, *const c_char, bool, *mut *const c_char) -> Res,
    create_stream: "OgaCreateTokenizerStream" => fn(*const c_void, *mut *mut c_void) -> Res,
    destroy_stream: "OgaDestroyTokenizerStream" => fn(*mut c_void),
    stream_decode: "OgaTokenizerStreamDecode" => fn(*mut c_void, i32, *mut *const c_char) -> Res,
    create_params: "OgaCreateGeneratorParams" => fn(*const c_void, *mut *mut c_void) -> Res,
    destroy_params: "OgaDestroyGeneratorParams" => fn(*mut c_void),
    params_set_number: "OgaGeneratorParamsSetSearchNumber" => fn(*mut c_void, *const c_char, f64) -> Res,
    params_set_bool: "OgaGeneratorParamsSetSearchBool" => fn(*mut c_void, *const c_char, bool) -> Res,
    create_generator: "OgaCreateGenerator" => fn(*const c_void, *const c_void, *mut *mut c_void) -> Res,
    destroy_generator: "OgaDestroyGenerator" => fn(*mut c_void),
    generator_append: "OgaGenerator_AppendTokens" => fn(*mut c_void, *const i32, usize) -> Res,
    generator_next: "OgaGenerator_GenerateNextToken" => fn(*mut c_void) -> Res,
    generator_is_done: "OgaGenerator_IsDone" => fn(*mut c_void) -> bool,
    generator_get_next_tokens: "OgaGenerator_GetNextTokens" => fn(*const c_void, *mut *const i32, *mut usize) -> Res,
    generator_token_count: "OgaGenerator_TokenCount" => fn(*const c_void) -> usize,
}

/// A loaded onnxruntime-genai.
pub struct Genai {
    api: Api,
    /// Kept loaded for as long as any function pointer may be called.
    _libs: Vec<Library>,
}

// SAFETY: the C API is thread safe for independent objects; the function table is read only.
unsafe impl Send for Genai {}
unsafe impl Sync for Genai {}

impl Genai {
    /// Loads `genai_lib`. `preload` are libraries it needs by bare name that must come from the
    /// managed folder (`DirectML.dll`, `onnxruntime.dll`), loaded first and by full path.
    pub fn load(genai_lib: &Path, preload: &[&Path]) -> Result<Arc<Self>, String> {
        let mut libs = Vec::new();
        for path in preload {
            // SAFETY: loading a library runs its initializers; these are the pinned Microsoft
            // binaries Harness downloaded.
            let lib = unsafe { Library::new(*path) }
                .map_err(|e| format!("cannot load {}: {}", path.display(), e))?;
            libs.push(lib);
        }
        // SAFETY: as above.
        let lib = unsafe { Library::new(genai_lib) }.map_err(|e| {
            format!(
                "cannot load onnxruntime-genai from {}: {}",
                genai_lib.display(),
                e
            )
        })?;
        let api = Api::load(&lib)?;
        libs.push(lib);
        Ok(Arc::new(Genai { api, _libs: libs }))
    }

    /// Turns an `OgaResult*` into `Ok(())` or the library's error message.
    fn check(&self, result: Res) -> Result<(), String> {
        if result.is_null() {
            return Ok(());
        }
        // SAFETY: a non-null result owns a NUL-terminated message until it is destroyed.
        let message = unsafe {
            let m = CStr::from_ptr((self.api.result_error)(result))
                .to_string_lossy()
                .into_owned();
            (self.api.destroy_result)(result);
            m
        };
        Err(message)
    }
}

/// The folder as genai should open it. A Windows path past `MAX_PATH` (a deep model folder plus
/// the long file names of Microsoft's repos) only opens in its `\?\` form, which `canonicalize`
/// returns; shorter paths are left alone (some code paths of the runtime dislike the prefix).
fn long_path_safe(dir: &Path) -> String {
    let plain = dir.to_string_lossy().to_string();
    if cfg!(windows) && plain.len() > 150 {
        if let Ok(full) = dir.canonicalize() {
            return full.to_string_lossy().to_string();
        }
    }
    plain
}

fn cstr(s: &str) -> Result<CString, String> {
    CString::new(s).map_err(|_| "text contains a NUL byte".to_string())
}

/// A loaded model with its tokenizer.
pub struct Model {
    lib: Arc<Genai>,
    model: *mut c_void,
    tokenizer: *mut c_void,
}

// SAFETY: genai models and tokenizers are used from several threads (one generator each).
unsafe impl Send for Model {}
unsafe impl Sync for Model {}

impl Model {
    /// Loads the model of `dir` (the folder with `genai_config.json`). `provider` replaces the
    /// providers of the config (`"cpu"`, `"dml"`, `"cuda"`) and gets `provider_options`; `None`
    /// keeps what the config says.
    pub fn load(
        lib: &Arc<Genai>,
        dir: &Path,
        provider: Option<&str>,
        provider_options: &[(&str, &str)],
    ) -> Result<Self, String> {
        let path = cstr(&long_path_safe(dir))?;
        let mut config = ptr::null_mut();
        // SAFETY: out pointers are valid; every created object is destroyed or owned below.
        unsafe {
            lib.check((lib.api.create_config)(path.as_ptr(), &mut config))?;
            let build = (|| -> Result<*mut c_void, String> {
                if let Some(p) = provider {
                    lib.check((lib.api.config_clear_providers)(config))?;
                    // The CPU is what remains with no provider.
                    if p != "cpu" {
                        let name = cstr(p)?;
                        lib.check((lib.api.config_append_provider)(config, name.as_ptr()))?;
                        // Options are keyed by the provider's canonical name (`DML`).
                        let canonical = cstr(&p.to_ascii_uppercase().replace("CUDA", "cuda"))?;
                        for (key, value) in provider_options {
                            let (key, value) = (cstr(key)?, cstr(value)?);
                            lib.check((lib.api.config_set_provider_option)(
                                config,
                                canonical.as_ptr(),
                                key.as_ptr(),
                                value.as_ptr(),
                            ))?;
                        }
                    }
                }
                let mut model = ptr::null_mut();
                lib.check((lib.api.create_model_from_config)(config, &mut model))?;
                Ok(model)
            })();
            (lib.api.destroy_config)(config);
            let model = build?;
            let mut tokenizer = ptr::null_mut();
            if let Err(e) = lib.check((lib.api.create_tokenizer)(model, &mut tokenizer)) {
                (lib.api.destroy_model)(model);
                return Err(e);
            }
            Ok(Model {
                lib: lib.clone(),
                model,
                tokenizer,
            })
        }
    }

    /// Token ids of `text`.
    pub fn encode(&self, text: &str) -> Result<Vec<i32>, String> {
        let text = cstr(text)?;
        let api = &self.lib.api;
        let mut seqs = ptr::null_mut();
        // SAFETY: `seqs` is created, read and destroyed here.
        unsafe {
            self.lib.check((api.create_sequences)(&mut seqs))?;
            let read = (|| {
                self.lib
                    .check((api.tokenizer_encode)(self.tokenizer, text.as_ptr(), seqs))?;
                let n = (api.sequences_count)(seqs, 0);
                let data = (api.sequences_data)(seqs, 0);
                Ok(if n == 0 || data.is_null() {
                    Vec::new()
                } else {
                    std::slice::from_raw_parts(data, n).to_vec()
                })
            })();
            (api.destroy_sequences)(seqs);
            read
        }
    }

    /// genai's own chat template (the tokenizer config's), for models whose template Harness
    /// cannot find as a file. `messages` is the JSON array of `{role, content}`.
    pub fn apply_chat_template(&self, messages: &str) -> Result<String, String> {
        let messages = cstr(messages)?;
        let api = &self.lib.api;
        let mut out: *const c_char = ptr::null();
        // SAFETY: the returned string is copied and destroyed.
        unsafe {
            self.lib.check((api.tokenizer_apply_template)(
                self.tokenizer,
                ptr::null(),
                messages.as_ptr(),
                ptr::null(),
                true,
                &mut out,
            ))?;
            let text = CStr::from_ptr(out).to_string_lossy().into_owned();
            (api.destroy_string)(out);
            Ok(text)
        }
    }

    /// Starts a generation over `prompt` (token ids).
    pub fn generate(
        &self,
        prompt: &[i32],
        options: &SearchOptions,
    ) -> Result<Generation<'_>, String> {
        let api = &self.lib.api;
        let mut params = ptr::null_mut();
        let mut generator = ptr::null_mut();
        let mut stream = ptr::null_mut();
        // SAFETY: every object created here is owned by the returned `Generation` or destroyed
        // on the error path.
        unsafe {
            self.lib
                .check((api.create_params)(self.model, &mut params))?;
            let setup = (|| -> Result<(), String> {
                let number = |name: &str, v: f64| -> Result<(), String> {
                    let n = cstr(name)?;
                    self.lib
                        .check((api.params_set_number)(params, n.as_ptr(), v))
                };
                number("max_length", options.max_length as f64)?;
                let sample = options.temperature > 0.0;
                let n = cstr("do_sample")?;
                self.lib
                    .check((api.params_set_bool)(params, n.as_ptr(), sample))?;
                if sample {
                    number("temperature", options.temperature)?;
                    if options.top_p > 0.0 && options.top_p < 1.0 {
                        number("top_p", options.top_p)?;
                    }
                    if options.top_k > 0 {
                        number("top_k", options.top_k as f64)?;
                    }
                    if let Some(seed) = options.seed {
                        number("random_seed", seed as f64)?;
                    }
                }
                if options.repetition_penalty > 0.0 && options.repetition_penalty != 1.0 {
                    number("repetition_penalty", options.repetition_penalty)?;
                }
                self.lib
                    .check((api.create_generator)(self.model, params, &mut generator))?;
                self.lib.check((api.generator_append)(
                    generator,
                    prompt.as_ptr(),
                    prompt.len(),
                ))?;
                self.lib
                    .check((api.create_stream)(self.tokenizer, &mut stream))
            })();
            (api.destroy_params)(params);
            if let Err(e) = setup {
                if !stream.is_null() {
                    (api.destroy_stream)(stream);
                }
                if !generator.is_null() {
                    (api.destroy_generator)(generator);
                }
                return Err(e);
            }
        }
        Ok(Generation {
            model: self,
            generator,
            stream,
        })
    }
}

impl Drop for Model {
    fn drop(&mut self) {
        // SAFETY: both handles are valid and destroyed once.
        unsafe {
            (self.lib.api.destroy_tokenizer)(self.tokenizer);
            (self.lib.api.destroy_model)(self.model);
        }
    }
}

/// Sampling settings of one generation.
#[derive(Debug, Clone, PartialEq)]
pub struct SearchOptions {
    /// Prompt tokens plus generated tokens (genai's `max_length`).
    pub max_length: usize,
    /// 0 means greedy decoding.
    pub temperature: f64,
    pub top_p: f64,
    pub top_k: i64,
    pub repetition_penalty: f64,
    pub seed: Option<u64>,
}

/// One running generation: pull tokens until [`Generation::is_done`].
pub struct Generation<'m> {
    model: &'m Model,
    generator: *mut c_void,
    stream: *mut c_void,
}

impl Generation<'_> {
    pub fn is_done(&self) -> bool {
        // SAFETY: valid generator.
        unsafe { (self.model.lib.api.generator_is_done)(self.generator) }
    }

    /// Tokens in the sequence so far (prompt included).
    pub fn token_count(&self) -> usize {
        // SAFETY: valid generator.
        unsafe { (self.model.lib.api.generator_token_count)(self.generator) }
    }

    /// Runs the model for one token and returns it with its text (possibly empty while the
    /// tokenizer holds back an incomplete character).
    pub fn next_token(&mut self) -> Result<(i32, String), String> {
        let lib = &self.model.lib;
        let api = &lib.api;
        // SAFETY: tokens point into the generator, the decoded text into the stream; both are
        // copied before the next call.
        unsafe {
            lib.check((api.generator_next)(self.generator))?;
            let mut tokens: *const i32 = ptr::null();
            let mut n = 0usize;
            lib.check((api.generator_get_next_tokens)(
                self.generator,
                &mut tokens,
                &mut n,
            ))?;
            if n == 0 || tokens.is_null() {
                return Err("the model produced no token".into());
            }
            let token = *tokens;
            let mut text: *const c_char = ptr::null();
            lib.check((api.stream_decode)(self.stream, token, &mut text))?;
            let text = if text.is_null() {
                String::new()
            } else {
                CStr::from_ptr(text).to_string_lossy().into_owned()
            };
            Ok((token, text))
        }
    }
}

impl Drop for Generation<'_> {
    fn drop(&mut self) {
        // SAFETY: both handles are valid and destroyed once.
        unsafe {
            (self.model.lib.api.destroy_stream)(self.stream);
            (self.model.lib.api.destroy_generator)(self.generator);
        }
    }
}
