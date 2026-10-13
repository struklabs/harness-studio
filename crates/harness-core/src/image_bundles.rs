//! Image model bundles: the files an image model needs besides its main file.
//!
//! A single-file checkpoint (SD1.x, SDXL, SD-Turbo) runs alone. A standalone diffusion model
//! (FLUX) needs a VAE and text encoders next to it, which live in other Hugging Face repos, so
//! a small curated list says which companions a known model takes, where they come from and the
//! generation settings it was made for (SDXL: 1024 x 1024, 25 steps, CFG 7; FLUX schnell: 4
//! steps, CFG 1). Everything here is stateless: a bundle is recognised by the main file's
//! *name* (the same for a Hub variant and a file on disk), and where a companion is found on
//! disk is derived from the download layout (`<library>/<owner>/<repo>/<file>`), so no pending
//! download has to be remembered.
//!
//! A model that is not in the list is not guessed: the user names its companions in the
//! definition (`vae`, `clip_l`, `clip_g`, `t5xxl`, `llm` of `params_sdcpp.toml`).
//!
//! The companions are the ones stable-diffusion.cpp's own docs name (docs/flux.md, docs/sd3.md),
//! from repos that need no license click: `ffxvs/vae-flux` carries the FLUX autoencoder
//! (`black-forest-labs/FLUX.1-*` answers 401 without a token), `comfyanonymous/flux_text_encoders`
//! the FLUX text encoders, `Comfy-Org/stable-diffusion-3.5-fp8` the SD3.5 ones; FLUX.2 klein takes
//! the FLUX.2 autoencoder of `Comfy-Org/flux2-dev` and a Qwen3 GGUF as its `llm` (docs/flux2.md).

use crate::params::{ParamValue, ParamValues};
use regex::Regex;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

/// One companion file of a bundle.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Component {
    /// Catalog id of `params_sdcpp.toml` (`vae`, `clip_l`, `clip_g`, `t5xxl`, `llm`).
    pub role: &'static str,
    /// Hugging Face repo.
    pub repo: &'static str,
    /// File path inside the repo (may be in a sub-folder).
    pub file: &'static str,
    /// Size in bytes (as the repo lists it; shown before anything is downloaded).
    pub size: u64,
}

/// Generation settings a model was made for (sd-server's own defaults are SD1.5's).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Defaults {
    pub width: u32,
    pub height: u32,
    pub steps: u32,
    pub cfg_scale: f64,
    pub sampling_method: Option<&'static str>,
    /// Distilled guidance (FLUX dev).
    pub guidance: Option<f64>,
}

/// A known image model and what it needs.
#[derive(Debug, Clone, Copy)]
pub struct Bundle {
    pub id: &'static str,
    pub label: &'static str,
    /// Pattern of the main file's name (case-insensitive).
    pub file_pattern: &'static str,
    pub components: &'static [Component],
    pub defaults: Defaults,
}

const FLUX_VAE: Component = Component {
    role: "vae",
    repo: "ffxvs/vae-flux",
    file: "ae.safetensors",
    size: 335_304_388,
};
const FLUX_CLIP_L: Component = Component {
    role: "clip_l",
    repo: "comfyanonymous/flux_text_encoders",
    file: "clip_l.safetensors",
    size: 246_144_152,
};
const FLUX_T5: Component = Component {
    role: "t5xxl",
    repo: "comfyanonymous/flux_text_encoders",
    file: "t5xxl_fp8_e4m3fn.safetensors",
    size: 4_893_934_904,
};
const FLUX2_VAE: Component = Component {
    role: "vae",
    repo: "Comfy-Org/flux2-dev",
    file: "split_files/vae/flux2-vae.safetensors",
    size: 336_213_556,
};
const QWEN3_4B: Component = Component {
    role: "llm",
    repo: "unsloth/Qwen3-4B-GGUF",
    file: "Qwen3-4B-Q4_K_M.gguf",
    size: 2_497_281_312,
};
const QWEN3_8B: Component = Component {
    role: "llm",
    repo: "unsloth/Qwen3-8B-GGUF",
    file: "Qwen3-8B-Q4_K_M.gguf",
    size: 5_027_784_512,
};
/// FLUX.2 klein (distilled): 4 steps without CFG; text-to-image and instruction editing.
const KLEIN_DEFAULTS: Defaults = Defaults {
    width: 1024,
    height: 1024,
    steps: 4,
    cfg_scale: 1.0,
    sampling_method: Some("euler"),
    guidance: None,
};
/// FLUX.2 klein base (undistilled): 20 steps at CFG 4.
const KLEIN_BASE_DEFAULTS: Defaults = Defaults {
    width: 1024,
    height: 1024,
    steps: 20,
    cfg_scale: 4.0,
    sampling_method: Some("euler"),
    guidance: None,
};
const SD3_CLIP_G: Component = Component {
    role: "clip_g",
    repo: "Comfy-Org/stable-diffusion-3.5-fp8",
    file: "text_encoders/clip_g.safetensors",
    size: 1_389_382_176,
};
const SD3_CLIP_L: Component = Component {
    role: "clip_l",
    repo: "Comfy-Org/stable-diffusion-3.5-fp8",
    file: "text_encoders/clip_l.safetensors",
    size: 246_144_152,
};
const SD3_T5: Component = Component {
    role: "t5xxl",
    repo: "Comfy-Org/stable-diffusion-3.5-fp8",
    file: "text_encoders/t5xxl_fp8_e4m3fn.safetensors",
    size: 4_893_934_904,
};

/// The curated list. Order matters: the first pattern that matches wins (`sdxl-turbo` before
/// `sdxl`, `sd-turbo` after both, a klein `base` before the distilled one).
pub const BUNDLES: &[Bundle] = &[
    Bundle {
        id: "flux2-klein-base-4b",
        label: "FLUX.2 klein base 4B",
        file_pattern: r"flux[-_.]?2[-_.]?klein[-_.]?base[-_.]?4b",
        components: &[FLUX2_VAE, QWEN3_4B],
        defaults: KLEIN_BASE_DEFAULTS,
    },
    Bundle {
        id: "flux2-klein-4b",
        label: "FLUX.2 klein 4B",
        file_pattern: r"flux[-_.]?2[-_.]?klein[-_.]?4b",
        components: &[FLUX2_VAE, QWEN3_4B],
        defaults: KLEIN_DEFAULTS,
    },
    Bundle {
        id: "flux2-klein-base-9b",
        label: "FLUX.2 klein base 9B",
        file_pattern: r"flux[-_.]?2[-_.]?klein[-_.]?base[-_.]?9b",
        components: &[FLUX2_VAE, QWEN3_8B],
        defaults: KLEIN_BASE_DEFAULTS,
    },
    Bundle {
        id: "flux2-klein-9b",
        label: "FLUX.2 klein 9B",
        file_pattern: r"flux[-_.]?2[-_.]?klein[-_.]?9b",
        components: &[FLUX2_VAE, QWEN3_8B],
        defaults: KLEIN_DEFAULTS,
    },
    Bundle {
        id: "flux-schnell",
        label: "FLUX.1 schnell",
        file_pattern: r"flux1?[-_.]?schnell",
        components: &[FLUX_VAE, FLUX_CLIP_L, FLUX_T5],
        defaults: Defaults {
            width: 1024,
            height: 1024,
            steps: 4,
            cfg_scale: 1.0,
            sampling_method: Some("euler"),
            guidance: None,
        },
    },
    Bundle {
        id: "flux-dev",
        label: "FLUX.1 dev",
        file_pattern: r"flux1?[-_.]?dev",
        components: &[FLUX_VAE, FLUX_CLIP_L, FLUX_T5],
        defaults: Defaults {
            width: 1024,
            height: 1024,
            steps: 20,
            cfg_scale: 1.0,
            sampling_method: Some("euler"),
            guidance: Some(3.5),
        },
    },
    Bundle {
        id: "sd35",
        label: "Stable Diffusion 3.5",
        file_pattern: r"sd3[._-]?5",
        components: &[SD3_CLIP_G, SD3_CLIP_L, SD3_T5],
        defaults: Defaults {
            width: 1024,
            height: 1024,
            steps: 28,
            cfg_scale: 4.5,
            sampling_method: Some("euler"),
            guidance: None,
        },
    },
    Bundle {
        id: "sdxl-turbo",
        label: "SDXL Turbo",
        file_pattern: r"sd[-_]?xl[-_]?turbo",
        components: &[],
        defaults: Defaults {
            width: 512,
            height: 512,
            steps: 4,
            cfg_scale: 1.0,
            sampling_method: Some("euler_a"),
            guidance: None,
        },
    },
    Bundle {
        id: "sdxl",
        label: "Stable Diffusion XL",
        file_pattern: r"sd[-_]?xl|stable-diffusion-xl",
        components: &[],
        defaults: Defaults {
            width: 1024,
            height: 1024,
            steps: 25,
            cfg_scale: 7.0,
            sampling_method: Some("euler_a"),
            guidance: None,
        },
    },
    Bundle {
        id: "sd-turbo",
        label: "SD-Turbo",
        file_pattern: r"sd[-_]?turbo",
        components: &[],
        defaults: Defaults {
            width: 512,
            height: 512,
            steps: 4,
            cfg_scale: 1.0,
            sampling_method: Some("euler_a"),
            guidance: None,
        },
    },
    Bundle {
        id: "sd15",
        label: "Stable Diffusion 1.5",
        file_pattern: r"stable-diffusion-v1-5|v1-5-pruned|sd[-_]?1[._]?5",
        components: &[],
        defaults: Defaults {
            width: 512,
            height: 512,
            steps: 20,
            cfg_scale: 7.5,
            sampling_method: Some("euler_a"),
            guidance: None,
        },
    },
];

fn patterns() -> &'static [Regex] {
    static RE: OnceLock<Vec<Regex>> = OnceLock::new();
    RE.get_or_init(|| {
        BUNDLES
            .iter()
            .map(|b| {
                Regex::new(&format!("(?i){}", b.file_pattern))
                    .expect("bundle patterns are valid (unit tested)")
            })
            .collect()
    })
}

/// The bundle a main file belongs to, by its file name (a path or a bare name).
pub fn bundle_for(file: &str) -> Option<&'static Bundle> {
    let name = file.rsplit(['/', '\\']).next().unwrap_or(file);
    // A VAE / text encoder / LoRA that happens to carry a model name is not a model.
    let lower = name.to_ascii_lowercase();
    if [
        "vae",
        "clip_",
        "t5xxl",
        "lora",
        "controlnet",
        "text_encoder",
    ]
    .iter()
    .any(|w| lower.starts_with(w) || lower.contains(&format!("_{w}")))
        || lower == "ae.safetensors"
    {
        return None;
    }
    BUNDLES
        .iter()
        .zip(patterns())
        .find(|(_, re)| re.is_match(name))
        .map(|(b, _)| b)
}

/// A file of a Hub repo that can be an image model or one of its companions: a `.safetensors`
/// or `.ckpt` at the repo root (or in the `text_encoders/` folder some repos keep theirs in).
/// Sub-folders are a diffusers layout's parts (`unet/`, `vae/`) and not loadable on their own.
pub fn is_image_candidate_file(path: &str) -> bool {
    let lower = path.to_ascii_lowercase();
    (lower.ends_with(".safetensors") || lower.ends_with(".ckpt"))
        && (!lower.contains('/') || lower.starts_with("text_encoders/"))
}

/// Whether a repo's file list is that of an image checkpoint repo rather than a language model
/// in safetensors (which has a root `config.json` and sharded `model-0000x-of-0000y` files): a
/// diffusers repo has `model_index.json`, a single-file repo has no `config.json`.
pub fn repo_has_image_files(paths: &[String]) -> bool {
    let has = |name: &str| paths.iter().any(|p| p.eq_ignore_ascii_case(name));
    paths.iter().any(|p| is_image_candidate_file(p))
        && (has("model_index.json") || !has("config.json"))
}

/// What a Hub variant of an image repo calls its precision: `FP16`, `BF16`, `FP8`, `FP32`, or
/// `component` for a VAE / text encoder (it is not a model of its own).
pub fn variant_quant(file: &str) -> String {
    let name = file.rsplit('/').next().unwrap_or(file).to_ascii_lowercase();
    if file.to_ascii_lowercase().starts_with("text_encoders/")
        || ["ae.", "vae", "clip_", "t5xxl", "umt5", "text_encoder"]
            .iter()
            .any(|w| name.starts_with(w))
    {
        return "component".to_string();
    }
    for (token, label) in [
        ("bf16", "BF16"),
        ("fp8", "FP8"),
        ("fp16", "FP16"),
        ("f16", "FP16"),
        ("fp32", "FP32"),
        ("f32", "FP32"),
    ] {
        if name.contains(token) {
            return label.to_string();
        }
    }
    "SAFETENSORS".to_string()
}

/// One companion of a plan: where it comes from and where it is on disk.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct ComponentPlan {
    pub role: String,
    pub repo: String,
    pub file: String,
    pub size: u64,
    /// The file on disk, when it is there.
    pub path: Option<String>,
}

/// What a main file needs (see [`plan_for`]).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct BundlePlan {
    pub bundle_id: String,
    pub label: String,
    pub components: Vec<ComponentPlan>,
    /// Generation settings the model was made for, as definition `load` values
    /// (`width`, `height`, `steps`, `cfg_scale`, ...).
    #[cfg_attr(feature = "openapi", schema(value_type = Object))]
    pub defaults: ParamValues,
}

impl BundlePlan {
    /// Every companion is on disk.
    pub fn complete(&self) -> bool {
        self.components.iter().all(|c| c.path.is_some())
    }

    /// Companions still to download.
    pub fn missing(&self) -> Vec<&ComponentPlan> {
        self.components
            .iter()
            .filter(|c| c.path.is_none())
            .collect()
    }

    /// The definition `load` settings the plan stands for: the companions found and the
    /// defaults.
    pub fn load(&self) -> ParamValues {
        let mut load = self.defaults.clone();
        for c in &self.components {
            if let Some(p) = &c.path {
                load.insert(c.role.clone(), ParamValue::Text(p.clone()));
            }
        }
        load
    }
}

fn defaults_load(d: &Defaults) -> ParamValues {
    let mut load = ParamValues::new();
    load.insert("width".into(), ParamValue::Int(d.width.into()));
    load.insert("height".into(), ParamValue::Int(d.height.into()));
    load.insert("steps".into(), ParamValue::Int(d.steps.into()));
    load.insert("cfg_scale".into(), ParamValue::Float(d.cfg_scale));
    if let Some(m) = d.sampling_method {
        load.insert("sampling_method".into(), ParamValue::Text(m.into()));
    }
    if let Some(g) = d.guidance {
        load.insert("guidance".into(), ParamValue::Float(g));
    }
    load
}

/// Where a companion lives under the download layout `<library>/<owner>/<repo>/<file>`, in the
/// first library folder that has it; or next to the main file (a model put together by hand).
pub fn find_component(
    c: &Component,
    libraries: &[PathBuf],
    main: Option<&Path>,
) -> Option<PathBuf> {
    for root in libraries {
        let p = root.join(c.repo).join(c.file);
        if p.is_file() {
            return Some(p);
        }
    }
    let name = c.file.rsplit('/').next().unwrap_or(c.file);
    main.and_then(Path::parent)
        .map(|dir| dir.join(name))
        .filter(|p| p.is_file())
}

/// The plan of a main file, or a bundle by id and a bare name: `libraries` are the model
/// folders companions are looked up in. `None` when the file is not in the curated list.
pub fn plan_for(file: &str, libraries: &[PathBuf], main: Option<&Path>) -> Option<BundlePlan> {
    let bundle = bundle_for(file)?;
    Some(BundlePlan {
        bundle_id: bundle.id.to_string(),
        label: bundle.label.to_string(),
        components: bundle
            .components
            .iter()
            .map(|c| ComponentPlan {
                role: c.role.to_string(),
                repo: c.repo.to_string(),
                file: c.file.to_string(),
                size: c.size,
                path: find_component(c, libraries, main).map(|p| p.to_string_lossy().to_string()),
            })
            .collect(),
        defaults: defaults_load(&bundle.defaults),
    })
}

/// Whether a main file is a standalone diffusion model that cannot run without companions it
/// has none of: a GGUF that names a diffusion architecture, or a safetensors file without the
/// single-file prefixes. Those are not given a definition by themselves.
pub fn needs_components(path: &Path) -> bool {
    use crate::engine::{MainRole, ModelFormat};
    match ModelFormat::of(path) {
        ModelFormat::Gguf => crate::models::cached_metadata(path)
            .and_then(|m| m.architecture)
            .is_some_and(|a| crate::engine::is_image_arch(&a)),
        ModelFormat::Safetensors => {
            crate::engine::sd_detect_main_role(path, false) == MainRole::Diffusion
        }
        _ => false,
    }
}

/// Definition `load` settings for a main file: the bundle's companions found on disk and its
/// defaults. `None` when the model cannot be given a definition yet: its companions are not all
/// there, or it needs some and is not a known bundle.
pub fn definition_load(
    path: &Path,
    libraries: &[PathBuf],
    require_complete: bool,
) -> Option<ParamValues> {
    let file = path.file_name()?.to_string_lossy().to_string();
    match plan_for(&file, libraries, Some(path)) {
        Some(plan) => (!require_complete || plan.complete()).then(|| plan.load()),
        None => (!needs_components(path)).then(ParamValues::new),
    }
}

/// Whether a finished download may complete an image model: a checkpoint or companion file, or
/// a GGUF that is a known image model.
pub fn concerns_image_models(file: &str) -> bool {
    is_image_candidate_file(file)
        || (file.to_ascii_lowercase().ends_with(".gguf") && bundle_for(file).is_some())
}

/// [`ensure_definitions`] over a fresh scan of the library (after a download finished).
pub fn sync_library_definitions() -> Result<Vec<String>, String> {
    ensure_definitions(&crate::models::ModelManager.scan_models())
}

/// Creates a definition for every image model of the library that has none and is ready to run
/// (companions present). Idempotent; returns the aliases it created. A model whose companions
/// are still downloading is picked up by a later call.
pub fn ensure_definitions(models: &[crate::models::LocalModel]) -> Result<Vec<String>, String> {
    crate::library_definitions::ensure(models, &[crate::domain::ModelKind::Image])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn known_models_are_recognised_by_file_name() {
        let id = |f: &str| bundle_for(f).map(|b| b.id);
        // The curated sources of the task: city96's FLUX.1-schnell GGUF, SDXL base, SD3.5.
        assert_eq!(id("flux1-schnell-Q4_K_S.gguf"), Some("flux-schnell"));
        assert_eq!(
            id("city96/FLUX.1-schnell-gguf/flux1-schnell-Q8_0.gguf"),
            Some("flux-schnell")
        );
        assert_eq!(id("flux1-schnell.safetensors"), Some("flux-schnell"));
        assert_eq!(id("flux1-dev-q4_0.gguf"), Some("flux-dev"));
        assert_eq!(id("sd_xl_base_1.0.safetensors"), Some("sdxl"));
        assert_eq!(id("sd_xl_turbo_1.0_fp16.safetensors"), Some("sdxl-turbo"));
        assert_eq!(id("sd3.5_medium.safetensors"), Some("sd35"));
        assert_eq!(id("sd3.5_large_fp8_scaled.safetensors"), Some("sd35"));
        assert_eq!(id("sd_turbo-f16-q8_0.gguf"), Some("sd-turbo"));
        assert_eq!(id("v1-5-pruned-emaonly.safetensors"), Some("sd15"));
        assert_eq!(
            id("stable-diffusion-v1-5-pruned-emaonly-Q8_0.gguf"),
            Some("sd15")
        );
        // FLUX.2 klein: leejet's / unsloth's GGUFs and Black Forest Labs' safetensors.
        assert_eq!(id("flux-2-klein-4b-Q4_0.gguf"), Some("flux2-klein-4b"));
        assert_eq!(
            id("unsloth/FLUX.2-klein-4B-GGUF/flux-2-klein-4b-Q4_K_M.gguf"),
            Some("flux2-klein-4b")
        );
        assert_eq!(id("flux-2-klein-4b.safetensors"), Some("flux2-klein-4b"));
        assert_eq!(
            id("flux-2-klein-base-4b-Q8_0.gguf"),
            Some("flux2-klein-base-4b")
        );
        assert_eq!(id("flux-2-klein-9b-Q4_0.gguf"), Some("flux2-klein-9b"));
        assert_eq!(
            id("flux-2-klein-base-9b.safetensors"),
            Some("flux2-klein-base-9b")
        );
        assert_eq!(id("flux2-vae.safetensors"), None);
        // Not in the list; companions are never mistaken for models.
        assert_eq!(id("qwen_image-Q4_K_M.gguf"), None);
        assert_eq!(id("Qwen3-4B-Q4_K_M.gguf"), None);
        for companion in [
            "ae.safetensors",
            "clip_l.safetensors",
            "t5xxl_fp8_e4m3fn.safetensors",
            "vae-flux.safetensors",
        ] {
            assert_eq!(id(companion), None, "{companion}");
        }
    }

    #[test]
    fn every_pattern_compiles_and_ids_are_unique() {
        assert_eq!(patterns().len(), BUNDLES.len());
        let mut seen = std::collections::BTreeSet::new();
        for b in BUNDLES {
            assert!(seen.insert(b.id), "duplicate {}", b.id);
            for c in b.components {
                assert!(
                    crate::engine::SD_COMPONENTS
                        .iter()
                        .any(|(id, _)| *id == c.role),
                    "{}: {} is not a component id",
                    b.id,
                    c.role
                );
                assert!(c.repo.contains('/') && !c.file.starts_with('/') && c.size > 0);
                assert!(!c.file.contains(".."), "{}", c.file);
            }
            assert!(b.defaults.steps > 0 && b.defaults.width >= 64);
        }
    }

    #[test]
    fn a_plan_finds_companions_in_the_download_layout() {
        let dir = tempfile::tempdir().unwrap();
        let lib = dir.path().to_path_buf();
        let main = lib.join("city96/FLUX.1-schnell-gguf/flux1-schnell-Q4_K_S.gguf");
        std::fs::create_dir_all(main.parent().unwrap()).unwrap();
        std::fs::write(&main, b"gguf").unwrap();
        let libs = vec![lib.clone()];
        let plan = plan_for("flux1-schnell-Q4_K_S.gguf", &libs, Some(&main)).unwrap();
        assert_eq!(plan.bundle_id, "flux-schnell");
        assert_eq!(
            plan.components
                .iter()
                .map(|c| c.role.as_str())
                .collect::<Vec<_>>(),
            ["vae", "clip_l", "t5xxl"]
        );
        assert!(!plan.complete());
        assert_eq!(plan.missing().len(), 3);
        // Nothing found yet: the load carries the defaults only (steps 4, CFG 1).
        let load = plan.load();
        assert_eq!(load.get("steps"), Some(&ParamValue::Int(4)));
        assert_eq!(load.get("cfg_scale"), Some(&ParamValue::Float(1.0)));
        assert_eq!(load.get("width"), Some(&ParamValue::Int(1024)));
        assert!(!load.contains_key("vae"));
        assert!(definition_load(&main, &libs, true).is_none());
        // The text encoders arrive: one more companion is found.
        for c in FLUX_CLIP_L.repo.split('/').next().into_iter() {
            let _ = c;
        }
        for comp in [FLUX_VAE, FLUX_CLIP_L, FLUX_T5] {
            let p = lib.join(comp.repo).join(comp.file);
            std::fs::create_dir_all(p.parent().unwrap()).unwrap();
            std::fs::write(&p, b"x").unwrap();
        }
        let plan = plan_for("flux1-schnell-Q4_K_S.gguf", &libs, Some(&main)).unwrap();
        assert!(plan.complete() && plan.missing().is_empty());
        let load = definition_load(&main, &libs, true).unwrap();
        let text = |id: &str| match load.get(id) {
            Some(ParamValue::Text(t)) => t.replace('\\', "/"),
            other => panic!("{id}: {other:?}"),
        };
        assert!(text("vae").ends_with("ffxvs/vae-flux/ae.safetensors"));
        assert!(text("clip_l").ends_with("comfyanonymous/flux_text_encoders/clip_l.safetensors"));
        assert!(text("t5xxl").ends_with("t5xxl_fp8_e4m3fn.safetensors"));
        // A hand-made folder: the companions next to the main file count too.
        let other = tempfile::tempdir().unwrap();
        let main2 = other.path().join("flux1-schnell-Q8_0.gguf");
        for name in [
            "flux1-schnell-Q8_0.gguf",
            "ae.safetensors",
            "clip_l.safetensors",
            "t5xxl_fp8_e4m3fn.safetensors",
        ] {
            std::fs::write(other.path().join(name), b"x").unwrap();
        }
        assert!(plan_for("flux1-schnell-Q8_0.gguf", &[], Some(&main2))
            .unwrap()
            .complete());
    }

    #[test]
    fn hub_repo_files_are_told_apart() {
        let files = |l: &[&str]| l.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        assert!(is_image_candidate_file("sd_xl_base_1.0.safetensors"));
        assert!(is_image_candidate_file("text_encoders/clip_g.safetensors"));
        assert!(is_image_candidate_file("v1-5-pruned.ckpt"));
        assert!(!is_image_candidate_file(
            "unet/diffusion_pytorch_model.safetensors"
        ));
        assert!(!is_image_candidate_file("flux1-schnell-Q4_K_S.gguf"));
        // A diffusers repo with a root checkpoint (sdxl-turbo) and a single-file repo count.
        assert!(repo_has_image_files(&files(&[
            "model_index.json",
            "config.json",
            "sd_xl_turbo_1.0_fp16.safetensors",
            "unet/x.safetensors"
        ])));
        assert!(repo_has_image_files(&files(&[
            "clip_l.safetensors",
            "t5xxl_fp8_e4m3fn.safetensors"
        ])));
        // A language model in safetensors does not.
        assert!(!repo_has_image_files(&files(&[
            "config.json",
            "model-00001-of-00002.safetensors",
            "model-00002-of-00002.safetensors"
        ])));
        assert_eq!(variant_quant("sd_xl_turbo_1.0_fp16.safetensors"), "FP16");
        assert_eq!(variant_quant("sd3.5_large_fp8_scaled.safetensors"), "FP8");
        assert_eq!(variant_quant("flux1-dev.safetensors"), "SAFETENSORS");
        assert_eq!(variant_quant("ae.safetensors"), "component");
        assert_eq!(variant_quant("t5xxl_fp8_e4m3fn.safetensors"), "component");
        assert_eq!(
            variant_quant("text_encoders/clip_g.safetensors"),
            "component"
        );
    }

    /// Real network: every companion of the curated list exists in its repo with the size the
    /// list states (the Hub's variant listing finds it by the same path).
    /// `cargo test -p harness-core --lib e2e_curated_companions_exist -- --ignored --nocapture`
    #[tokio::test]
    #[ignore]
    async fn e2e_curated_companions_exist() {
        use crate::downloader::DownloadManager;
        let mut seen = std::collections::BTreeSet::new();
        for bundle in BUNDLES {
            for c in bundle.components {
                if !seen.insert((c.repo, c.file)) {
                    continue;
                }
                let variants = DownloadManager::get_model_variants(c.repo, None)
                    .await
                    .unwrap_or_else(|e| panic!("{}: {e}", c.repo));
                let found = variants
                    .iter()
                    .flat_map(|v| &v.shards)
                    .find(|s| s.filename == c.file)
                    .unwrap_or_else(|| panic!("{}/{} is not listed", c.repo, c.file));
                eprintln!(
                    "{}/{}: {} bytes (list says {})",
                    c.repo, c.file, found.size_bytes, c.size
                );
                assert_eq!(found.size_bytes, c.size, "{}/{}", c.repo, c.file);
            }
        }
    }

    /// A real image model in a library folder gets its definition (the curated defaults of its
    /// bundle) once, and a second sync changes nothing.
    /// `HS_SD_MODEL=<sd_turbo-*.gguf> cargo test -p harness-core --lib e2e_library_sync -- --ignored --nocapture`
    #[test]
    #[ignore]
    fn e2e_library_sync() {
        let Some(model) = std::env::var_os("HS_SD_MODEL")
            .map(PathBuf::from)
            .filter(|p| p.is_file())
        else {
            eprintln!("HS_SD_MODEL not set, skipping");
            return;
        };
        let dir = tempfile::Builder::new()
            .prefix(&crate::paths::test_dir_prefix("img-sync"))
            .tempdir()
            .unwrap()
            .keep();
        crate::paths::init(dir.clone());
        let mut settings = crate::config::AppSettings::load();
        settings.extra_models_dirs = vec![model.parent().unwrap().to_string_lossy().to_string()];
        settings.save().unwrap();

        let models = crate::models::ModelManager.scan_models();
        let found = models
            .iter()
            .find(|m| m.path == model)
            .expect("the model is in the library");
        assert_eq!(found.kind, crate::domain::ModelKind::Image, "{found:?}");
        let made = sync_library_definitions().unwrap();
        eprintln!("created: {made:?}");
        assert_eq!(made.len(), 1);
        let defs = crate::domain::store::Domain::global()
            .definitions()
            .unwrap();
        let def = defs
            .iter()
            .find(|d| d.base.local_path() == Some(model.as_path()))
            .unwrap();
        assert_eq!(def.load.get("steps"), Some(&ParamValue::Int(4)));
        assert_eq!(def.load.get("width"), Some(&ParamValue::Int(512)));
        assert!(sync_library_definitions().unwrap().is_empty());
    }

    #[test]
    fn single_file_models_need_nothing() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("sd_turbo-f16-q8_0.gguf");
        std::fs::write(&p, b"x").unwrap();
        let plan = plan_for("sd_turbo-f16-q8_0.gguf", &[], Some(&p)).unwrap();
        assert!(plan.complete() && plan.components.is_empty());
        let load = definition_load(&p, &[], true).unwrap();
        assert_eq!(load.get("steps"), Some(&ParamValue::Int(4)));
        assert_eq!(
            load.get("sampling_method"),
            Some(&ParamValue::Text("euler_a".into()))
        );
        // A model outside the list that is a checkpoint (not a bare diffusion model) is made a
        // definition with no settings.
        let other = dir.path().join("my-finetune.ckpt");
        std::fs::write(&other, b"x").unwrap();
        assert_eq!(definition_load(&other, &[], true), Some(ParamValues::new()));
    }
}
