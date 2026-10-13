//! The architectures a converter (`convert_hf_to_gguf.py` of one llama.cpp tag) can handle.
//!
//! The converter registers its model classes with `@ModelBase.register("LlamaForCausalLM", ...)`
//! (in `convert_hf_to_gguf.py` and the `conversion/` package). Reading those decorators out of
//! the pinned source answers "can this repo be converted?" before any Python runs, from the
//! very script that would do the work. A class derived (directly or through other classes) from
//! `MmprojModel` writes the vision projector (`--mmproj`); every other class writes the text
//! model. The `quant_method`s of pre-quantized checkpoints it dequantizes are read the same way
//! (`quant_method == "gptq"`...): a repo quantized with any other method (AWQ, bitsandbytes...)
//! would download in full and then fail.

use regex::Regex;
use std::collections::{BTreeSet, HashMap};

/// What one converter registers.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Architectures {
    /// `architectures[0]` names of config.json the text converter knows.
    pub text: BTreeSet<String>,
    /// Names the `--mmproj` converter knows (vision encoders).
    pub vision: BTreeSet<String>,
    /// `quantization_config.quant_method` values the converter dequantizes (`fp8`, `gptq`...).
    pub quant_methods: BTreeSet<String>,
}

impl Architectures {
    pub fn supports_text(&self, arch: &str) -> bool {
        self.text.contains(arch)
    }

    pub fn supports_vision(&self, arch: &str) -> bool {
        self.vision.contains(arch)
    }

    /// Whether a checkpoint quantized with `method` converts. Unknown when the source names no
    /// method at all (an older converter): then nothing is refused.
    pub fn supports_quant(&self, method: &str) -> bool {
        self.quant_methods.is_empty() || self.quant_methods.contains(&method.to_ascii_lowercase())
    }
}

const MMPROJ_BASE: &str = "MmprojModel";

/// Reads the registrations out of the converter's Python sources (all `.py` files of the
/// script and its `conversion/` package, concatenated or one by one).
pub fn scan<S: AsRef<str>>(sources: &[S]) -> Architectures {
    // A registration, and the class it decorates: the first `class` after it (other
    // decorators, comments and further registrations stacked on the same class come between).
    let register = Regex::new(r#"(?m)^@ModelBase\.register\(([^()]*)\)"#).expect("static regex");
    let next_def = Regex::new(r"(?m)^(class|def)\s+(\w+)").expect("static regex");
    let class = Regex::new(r"(?m)^class\s+(\w+)\s*\(([^)]*)\)").expect("static regex");
    let name = Regex::new(r#""([^"]+)""#).expect("static regex");
    let quant = Regex::new(r#"quant_method\s*==\s*["']([\w-]+)["']"#).expect("static regex");

    let mut bases: HashMap<String, Vec<String>> = HashMap::new();
    for src in sources {
        for c in class.captures_iter(src.as_ref()) {
            bases.insert(c[1].to_string(), base_names(&c[2]));
        }
    }
    let is_mmproj = |start: &str| -> bool {
        let mut seen = BTreeSet::new();
        let mut stack = vec![start.to_string()];
        while let Some(cls) = stack.pop() {
            if cls == MMPROJ_BASE {
                return true;
            }
            if !seen.insert(cls.clone()) {
                continue;
            }
            if let Some(parents) = bases.get(&cls) {
                stack.extend(parents.iter().cloned());
            }
        }
        false
    };

    let mut found = Architectures::default();
    for src in sources {
        let src = src.as_ref();
        for q in quant.captures_iter(src) {
            found.quant_methods.insert(q[1].to_ascii_lowercase());
        }
        for m in register.captures_iter(src) {
            let end = m.get(0).map_or(0, |g| g.end());
            // A function (a hparams loader, a helper) is not a model class.
            let Some(c) = next_def.captures(&src[end..]) else {
                continue;
            };
            if &c[1] != "class" {
                continue;
            }
            let vision = is_mmproj(&c[2]);
            for n in name.captures_iter(&m[1]) {
                let target = if vision {
                    &mut found.vision
                } else {
                    &mut found.text
                };
                target.insert(n[1].to_string());
            }
        }
    }
    found
}

/// `Base1, Base2[T], metaclass=X` -> `["Base1", "Base2"]`.
fn base_names(list: &str) -> Vec<String> {
    list.split(',')
        .map(|b| b.split('[').next().unwrap_or("").trim())
        .filter(|b| !b.is_empty() && !b.contains('='))
        .map(|b| b.rsplit('.').next().unwrap_or(b).to_string())
        .collect()
}

/// Every `.py` file of a converter source folder (`convert_hf_to_gguf.py` and `conversion/`).
pub fn scan_dir(src: &std::path::Path) -> std::io::Result<Architectures> {
    let mut texts = vec![std::fs::read_to_string(src.join("convert_hf_to_gguf.py"))?];
    if let Ok(dir) = std::fs::read_dir(src.join("conversion")) {
        for entry in dir.flatten() {
            let path = entry.path();
            if path.extension().is_some_and(|e| e == "py") {
                if let Ok(text) = std::fs::read_to_string(&path) {
                    texts.push(text);
                }
            }
        }
    }
    Ok(scan(&texts))
}
