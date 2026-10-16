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
//! FLUX.2 dev takes the same autoencoder with Mistral Small 3.2 as `llm`; Qwen-Image takes its
//! own VAE (`Comfy-Org/Qwen-Image_ComfyUI`) and Qwen2.5-VL (docs/qwen_image.md,
//! docs/qwen_image_edit.md), Qwen-Image 2.1 its VAE (`Comfy-Org/Qwen-Image-2.1`) and Qwen3-VL-8B
//! (docs/qwen_image_2.1.md), Z-Image the FLUX.1 autoencoder and Qwen3-4B-Instruct
//! (docs/z_image.md), FLUX.1 Kontext the FLUX.1 set (docs/kontext.md).

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
const FLUX2_MISTRAL: Component = Component {
    role: "llm",
    repo: "unsloth/Mistral-Small-3.2-24B-Instruct-2506-GGUF",
    file: "Mistral-Small-3.2-24B-Instruct-2506-Q4_K_M.gguf",
    size: 14_333_922_848,
};
const QWEN_IMAGE_VAE: Component = Component {
    role: "vae",
    repo: "Comfy-Org/Qwen-Image_ComfyUI",
    file: "split_files/vae/qwen_image_vae.safetensors",
    size: 253_806_246,
};
const QWEN25_VL_7B: Component = Component {
    role: "llm",
    repo: "mradermacher/Qwen2.5-VL-7B-Instruct-GGUF",
    file: "Qwen2.5-VL-7B-Instruct.Q4_K_M.gguf",
    size: 4_683_072_512,
};
/// The vision projector of Qwen2.5-VL; Qwen-Image-Edit 2509 reads the reference image with it.
const QWEN25_VL_7B_MMPROJ: Component = Component {
    role: "llm_vision",
    repo: "mradermacher/Qwen2.5-VL-7B-Instruct-GGUF",
    file: "Qwen2.5-VL-7B-Instruct.mmproj-Q8_0.gguf",
    size: 853_119_712,
};
const QWEN_IMAGE_21_VAE: Component = Component {
    role: "vae",
    repo: "Comfy-Org/Qwen-Image-2.1",
    file: "vae/qwen_image_2.1_vae_bf16.safetensors",
    size: 675_509_688,
};
const QWEN3_VL_8B: Component = Component {
    role: "llm",
    repo: "Qwen/Qwen3-VL-8B-Instruct-GGUF",
    file: "Qwen3VL-8B-Instruct-Q4_K_M.gguf",
    size: 5_027_784_800,
};
const QWEN3_4B_INSTRUCT: Component = Component {
    role: "llm",
    repo: "unsloth/Qwen3-4B-Instruct-2507-GGUF",
    file: "Qwen3-4B-Instruct-2507-Q4_K_M.gguf",
    size: 2_497_281_120,
};
/// Qwen-Image / Qwen-Image-Edit (docs/qwen_image.md): CFG 2.5, euler.
const QWEN_IMAGE_DEFAULTS: Defaults = Defaults {
    width: 1024,
    height: 1024,
    steps: 20,
    cfg_scale: 2.5,
    sampling_method: Some("euler"),
    guidance: None,
};
/// Qwen-Image 2.1 (docs/qwen_image_2.1.md): CFG 6, euler.
const QWEN_IMAGE_21_DEFAULTS: Defaults = Defaults {
    width: 1024,
    height: 1024,
    steps: 20,
    cfg_scale: 6.0,
    sampling_method: Some("euler"),
    guidance: None,
};
/// Z-Image Turbo: 8 steps without CFG (docs/z_image.md).
const Z_IMAGE_TURBO_DEFAULTS: Defaults = Defaults {
    width: 1024,
    height: 1024,
    steps: 8,
    cfg_scale: 1.0,
    sampling_method: None,
    guidance: None,
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
const WAN21_VAE: Component = Component {
    role: "vae",
    repo: "Comfy-Org/Wan_2.1_ComfyUI_repackaged",
    file: "split_files/vae/wan_2.1_vae.safetensors",
    size: 253_815_318,
};
/// umt5-xxl at Q3_K_S: with Wan2.1 1.3B it fits an 8 GB Apple M1 (5.4 GB of Metal memory).
const UMT5_XXL: Component = Component {
    role: "t5xxl",
    repo: "city96/umt5-xxl-encoder-gguf",
    file: "umt5-xxl-encoder-Q3_K_S.gguf",
    size: 2_858_489_696,
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
        id: "flux2-dev",
        label: "FLUX.2 dev",
        file_pattern: r"flux[-_.]?2[-_.]?dev",
        components: &[FLUX2_VAE, FLUX2_MISTRAL],
        defaults: Defaults {
            steps: 28,
            cfg_scale: 1.0,
            ..KLEIN_BASE_DEFAULTS
        },
    },
    Bundle {
        id: "qwen-image-2.1-turbo",
        label: "Qwen-Image 2.1 Turbo",
        file_pattern: r"qwen[-_.]?image[-_.]?2[-_.]?1[-_.](?:\w+[-_.])*?turbo",
        components: &[QWEN_IMAGE_21_VAE, QWEN3_VL_8B],
        defaults: Defaults {
            steps: 8,
            cfg_scale: 1.0,
            ..QWEN_IMAGE_21_DEFAULTS
        },
    },
    Bundle {
        id: "qwen-image-2.1",
        label: "Qwen-Image 2.1",
        file_pattern: r"qwen[-_.]?image[-_.]?2[-_.]?1(?:[-_.]|$)",
        components: &[QWEN_IMAGE_21_VAE, QWEN3_VL_8B],
        defaults: QWEN_IMAGE_21_DEFAULTS,
    },
    Bundle {
        id: "qwen-image-edit-2509",
        label: "Qwen-Image Edit 2509",
        file_pattern: r"qwen[-_.]?image[-_.]?edit[-_.]?2509",
        components: &[QWEN_IMAGE_VAE, QWEN25_VL_7B, QWEN25_VL_7B_MMPROJ],
        defaults: QWEN_IMAGE_DEFAULTS,
    },
    Bundle {
        id: "qwen-image-edit-2511",
        label: "Qwen-Image Edit 2511",
        file_pattern: r"qwen[-_.]?image[-_.]?edit[-_.]?2511",
        components: &[QWEN_IMAGE_VAE, QWEN25_VL_7B, QWEN25_VL_7B_MMPROJ],
        defaults: QWEN_IMAGE_DEFAULTS,
    },
    Bundle {
        id: "qwen-image-edit",
        label: "Qwen-Image Edit",
        file_pattern: r"qwen[-_.]?image[-_.]?edit",
        components: &[QWEN_IMAGE_VAE, QWEN25_VL_7B, QWEN25_VL_7B_MMPROJ],
        defaults: QWEN_IMAGE_DEFAULTS,
    },
    Bundle {
        id: "qwen-image",
        label: "Qwen-Image",
        file_pattern: r"qwen[-_.]?image",
        components: &[QWEN_IMAGE_VAE, QWEN25_VL_7B],
        defaults: QWEN_IMAGE_DEFAULTS,
    },
    Bundle {
        id: "z-image-turbo",
        label: "Z-Image Turbo",
        file_pattern: r"z[-_.]?image[-_.]?turbo",
        components: &[FLUX_VAE, QWEN3_4B_INSTRUCT],
        defaults: Z_IMAGE_TURBO_DEFAULTS,
    },
    Bundle {
        id: "z-image",
        label: "Z-Image",
        file_pattern: r"z[-_.]?image",
        components: &[FLUX_VAE, QWEN3_4B_INSTRUCT],
        defaults: Defaults {
            steps: 20,
            cfg_scale: 5.0,
            ..Z_IMAGE_TURBO_DEFAULTS
        },
    },
    Bundle {
        id: "flux-kontext-dev",
        label: "FLUX.1 Kontext dev",
        file_pattern: r"flux1?[-_.]?kontext",
        components: &[FLUX_VAE, FLUX_CLIP_L, FLUX_T5],
        defaults: Defaults {
            width: 1024,
            height: 1024,
            steps: 20,
            cfg_scale: 1.0,
            sampling_method: Some("euler"),
            guidance: Some(2.5),
        },
    },
    Bundle {
        id: "wan21-t2v-1.3b",
        label: "Wan2.1 T2V 1.3B",
        file_pattern: r"wan2[._]?1[-_.]t2v[-_.]1[._]3b",
        components: &[WAN21_VAE, UMT5_XXL],
        // Wan's own size and settings (stable-diffusion.cpp docs/wan.md).
        defaults: Defaults {
            width: 832,
            height: 480,
            steps: 20,
            cfg_scale: 6.0,
            sampling_method: Some("euler"),
            guidance: None,
        },
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
    if is_adapter_file(file) || crate::hub_compat::is_sdxl_refiner("", &[file.to_string()]) {
        return None;
    }
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
    .any(|w| {
        lower.starts_with(w) || lower.contains(&format!("_{w}")) || lower.contains(&format!("-{w}"))
    }) || lower == "ae.safetensors"
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

/// Audited full Lightning exports coexist with adapters whose names omit the scaled precision.
pub fn is_standalone_checkpoint(path: &str) -> bool {
    let name = path.to_ascii_lowercase();
    matches!(
        name.as_str(),
        "qwen_image_edit_2511_fp8_e4m3fn_scaled.safetensors"
            | "qwen_image_edit_2511_fp8_e4m3fn_scaled_lightning_4steps_v1.0.safetensors"
            | "qwen_image_edit_2511_fp8_e4m3fn_scaled_lightning_8steps_v1.0.safetensors"
            | "qwen_image_edit_2511_fp8_e4m3fn_scaled_lightning_comfyui_4steps_v1.0.safetensors"
    )
}

/// Full Lightning / scaled fp8 exports of Qwen-Image-Edit 2511 whose `weight_scale` tensors have
/// the diffusers layout `[N, 1]`: stable-diffusion.cpp stops at load time ("wrong shape in model
/// metadata"). The ComfyUI export of the same weights (`scale_weight`, scalar) loads.
pub fn is_unloadable_scaled_fp8(path: &str) -> bool {
    let name = path.rsplit('/').next().unwrap_or(path).to_ascii_lowercase();
    matches!(
        name.as_str(),
        "qwen_image_edit_2511_fp8_e4m3fn_scaled.safetensors"
            | "qwen_image_edit_2511_fp8_e4m3fn_scaled_lightning_4steps_v1.0.safetensors"
            | "qwen_image_edit_2511_fp8_e4m3fn_scaled_lightning_8steps_v1.0.safetensors"
    )
}

pub fn is_adapter_file(path: &str) -> bool {
    let name = path.rsplit('/').next().unwrap_or(path).to_ascii_lowercase();
    name.split(|c: char| !c.is_ascii_alphanumeric())
        .any(|t| t == "lora")
        || (name.starts_with("qwen-image-edit-2511-lightning-") && name.ends_with(".safetensors"))
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
/// `component` for a VAE / text encoder / LoRA adapter (it is not a model of its own).
pub fn variant_quant(file: &str) -> String {
    let name = file.rsplit('/').next().unwrap_or(file).to_ascii_lowercase();
    if is_adapter_file(file)
        || file.to_ascii_lowercase().starts_with("text_encoders/")
        || ["ae.", "vae", "clip_", "t5xxl", "umt5", "text_encoder"]
            .iter()
            .any(|w| name.starts_with(w))
        || name
            .split(|c: char| !c.is_ascii_alphanumeric())
            .any(|token| token == "lora")
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
    // A repo that ships its own vision projector (next to the main file or in the sibling
    // `text_encoders/` folder) is used with it, rather than the curated one.
    if c.role == "llm_vision" {
        if let Some(own) = main.and_then(|p| own_projector(p, libraries)) {
            return Some(own);
        }
    }
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

/// Whether a main file is an instruction-editing model: it reads the picture(s) as references
/// and draws the edit from noise (FLUX.1 Kontext, FLUX.2 klein / dev, Qwen-Image-Edit).
pub fn is_reference_edit_file(file: &str) -> bool {
    bundle_for(file).is_some_and(|b| {
        b.id.starts_with("flux2-")
            || b.id.starts_with("qwen-image-edit")
            || b.id == "flux-kontext-dev"
    })
}

/// Whether a repo file is a vision projector (`mmproj`) GGUF.
pub fn is_projector_file(path: &str) -> bool {
    let lower = path.to_ascii_lowercase();
    lower.ends_with(".gguf") && lower.contains("mmproj")
}

/// How much a projector precision is preferred (lower is better): full precision first.
pub fn projector_rank(path: &str) -> u8 {
    let lower = path.to_ascii_lowercase();
    if lower.contains("f16") && !lower.contains("bf16") {
        0
    } else if lower.contains("bf16") {
        1
    } else if lower.contains("q8_0") {
        2
    } else {
        3
    }
}

/// Select a compatible repo projector using the same layout and family rules for downloads
/// and disk discovery. Paths must use forward slashes and be relative to the same root.
pub fn select_own_projector<'a>(
    main: &str,
    candidates: impl IntoIterator<Item = &'a str>,
) -> Option<&'a str> {
    let curated = bundle_for(main)?
        .components
        .iter()
        .find(|c| c.role == "llm_vision")?;
    // Drop the parameter count (7B) and suffixes, retaining the architecture family.
    let family = curated
        .file
        .rsplit('/')
        .next()?
        .split(['-', '_'])
        .take_while(|part| {
            !part.ends_with(['B', 'b'])
                || !part[..part.len() - 1].chars().all(|c| c.is_ascii_digit())
        })
        .collect::<String>();
    let normalize = |s: &str| {
        s.chars()
            .filter(char::is_ascii_alphanumeric)
            .flat_map(char::to_lowercase)
            .collect::<String>()
    };
    let family = normalize(&family);
    let dirs = projector_dirs(Path::new(main))?;
    candidates
        .into_iter()
        .filter(|p| {
            let name = p.rsplit('/').next().unwrap_or(p);
            let normalized_name = normalize(name);
            is_projector_file(name)
                && normalized_name.match_indices(&family).any(|(start, _)| {
                    // Locate the last matched ASCII character in the original name:
                    // normalization removes separators, but the boundary must not.
                    name.char_indices()
                        .filter(|(_, c)| c.is_ascii_alphanumeric())
                        .nth(start + family.len() - 1)
                        .is_some_and(|(end, c)| {
                            name[end + c.len_utf8()..]
                                .chars()
                                .next()
                                .is_none_or(|c| !c.is_alphanumeric())
                        })
                })
                && Path::new(p)
                    .parent()
                    .is_some_and(|d| dirs.iter().any(|allowed| d == allowed))
        })
        .min_by_key(|p| (projector_rank(p), p.to_ascii_lowercase()))
}

fn projector_dirs(main: &Path) -> Option<[PathBuf; 3]> {
    let dir = main.parent()?;
    let parent = dir.parent().unwrap_or_else(|| Path::new(""));
    Some([
        dir.to_path_buf(),
        parent.join("text_encoders"),
        parent.join("text_encoder"),
    ])
}

/// Find the repo's compatible projector beside the main file or in sibling text encoder folders.
pub fn own_projector(main: &Path, libraries: &[PathBuf]) -> Option<PathBuf> {
    let dir = main.parent()?;
    let repo_root = libraries.iter().find_map(|library| {
        let relative = main.strip_prefix(library).ok()?;
        let mut parts = relative.components();
        let root = library
            .join(parts.next()?.as_os_str())
            .join(parts.next()?.as_os_str());
        parts.next()?; // A repo root must still contain a file path.
        Some(root)
    });
    let root = repo_root.as_deref().unwrap_or_else(|| {
        if dir.file_name().is_some_and(|n| n == "diffusion_models") {
            dir.parent().unwrap_or(dir)
        } else {
            dir
        }
    });
    let relative = main.strip_prefix(root).ok()?;
    let dirs = projector_dirs(relative)?;
    let candidates: Vec<String> = dirs
        .into_iter()
        .flat_map(|d| {
            std::fs::read_dir(root.join(d))
                .into_iter()
                .flatten()
                .flatten()
        })
        .map(|e| e.path())
        .filter(|p| p.is_file())
        .filter_map(|p| {
            p.strip_prefix(root)
                .ok()
                .map(|p| p.to_string_lossy().replace('\\', "/"))
        })
        .collect();
    let main = main
        .strip_prefix(root)
        .ok()?
        .to_string_lossy()
        .replace('\\', "/");
    select_own_projector(&main, candidates.iter().map(String::as_str)).map(|p| root.join(p))
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
        defaults: {
            let mut load = defaults_load(&bundle.defaults);
            if bundle.id == "qwen-image-edit-2511" {
                load.insert(
                    "model_args".into(),
                    ParamValue::Text("qwen_image_zero_cond_t=true".into()),
                );
            }
            load
        },
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
        || is_bundle_companion(file)
        || is_projector_file(file)
}

/// Whether `file` is one of the curated companions of a bundle (a text encoder or projector GGUF
/// is the last file a model waits for, and has no image-model name of its own).
pub fn is_bundle_companion(file: &str) -> bool {
    let lower = file.replace('\\', "/").to_ascii_lowercase();
    BUNDLES.iter().flat_map(|b| b.components).any(|c| {
        let want = format!("{}/{}", c.repo, c.file).to_ascii_lowercase();
        lower == want || lower.ends_with(&format!("/{want}"))
    })
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
        assert_eq!(
            id("wan2.1_t2v_1.3B_fp16.safetensors"),
            Some("wan21-t2v-1.3b")
        );
        assert_eq!(id("wan2.1-t2v-1.3b-Q8_0.gguf"), Some("wan21-t2v-1.3b"));
        assert_eq!(id("wan_2.1_vae.safetensors"), None);
        assert_eq!(id("umt5-xxl-encoder-Q3_K_S.gguf"), None);
        // Not in the list; companions are never mistaken for models.
        assert_eq!(id("Qwen3-VL-8B-Instruct-Q4_K_M.gguf"), None);
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
    fn qwen_z_image_and_flux2_dev_take_their_documented_companions() {
        let plan = |f: &str| {
            let b = bundle_for(f).unwrap_or_else(|| panic!("{f} has no bundle"));
            (
                b.id,
                b.components
                    .iter()
                    .map(|c| (c.role, c.repo, c.file))
                    .collect::<Vec<_>>(),
            )
        };
        // Qwen-Image 2.1 (and its uncensored finetunes) brings its own VAE and Qwen3-VL.
        for f in [
            "qwen-image-2.1-Q4_K_M.gguf",
            "qwen-image-2.1-UC-Q4_K_M.gguf",
            "qwen_image_2.1_int8_convrot.safetensors",
        ] {
            let (id, comps) = plan(f);
            assert_eq!(id, "qwen-image-2.1", "{f}");
            assert_eq!(comps[0].1, "Comfy-Org/Qwen-Image-2.1");
            assert_eq!(comps[1].1, "Qwen/Qwen3-VL-8B-Instruct-GGUF");
        }
        for f in [
            "qwen-image-2.1-turbo-Q8_0.gguf",
            "Qwen-Image-2.1-Turbo-Uncensored-AD-Q4_K.gguf",
        ] {
            assert_eq!(plan(f).0, "qwen-image-2.1-turbo", "{f}");
        }
        let turbo = bundle_for("qwen-image-2.1-turbo-Q8_0.gguf").unwrap();
        assert_eq!((turbo.defaults.steps, turbo.defaults.cfg_scale), (8, 1.0));
        // The earlier Qwen-Image releases and the edit models use the Qwen2.5-VL set.
        assert_eq!(plan("qwen-image-2512-Q4_K_M.gguf").0, "qwen-image");
        assert_eq!(plan("qwen_image_bf16.safetensors").0, "qwen-image");
        assert_eq!(
            plan("qwen-image-edit-2511-Q4_K_M.gguf").0,
            "qwen-image-edit-2511"
        );
        assert_eq!(
            plan("qwen-image-edit-2511-uncensored-Q4_K_M.gguf").0,
            "qwen-image-edit-2511"
        );
        let (id, comps) = plan("Qwen-Image-Edit-2509-Q4_K_M.gguf");
        assert_eq!(id, "qwen-image-edit-2509");
        assert_eq!(
            comps.iter().map(|c| c.0).collect::<Vec<_>>(),
            ["vae", "llm", "llm_vision"]
        );
        // Z-Image: the FLUX.1 autoencoder and Qwen3-4B-Instruct.
        let (id, comps) = plan("z-image-turbo-Q4_K_M.gguf");
        assert_eq!(id, "z-image-turbo");
        assert_eq!(comps[0].1, "ffxvs/vae-flux");
        assert_eq!(comps[1].2, "Qwen3-4B-Instruct-2507-Q4_K_M.gguf");
        assert_eq!(plan("z_image_bf16.safetensors").0, "z-image");
        // FLUX.2 dev and FLUX.1 Kontext do not fall into the neighbouring FLUX bundles.
        let (id, comps) = plan("flux2-dev-Q4_K_S.gguf");
        assert_eq!(id, "flux2-dev");
        assert_eq!(comps[0].1, "Comfy-Org/flux2-dev");
        assert_eq!(
            comps[1].1,
            "unsloth/Mistral-Small-3.2-24B-Instruct-2506-GGUF"
        );
        assert_eq!(plan("flux1-kontext-dev-Q4_K_M.gguf").0, "flux-kontext-dev");
        assert_eq!(plan("flux1-dev-q4_0.gguf").0, "flux-dev");
        // Companions and LoRAs that carry a model name are not models.
        for f in [
            "qwen_image_vae.safetensors",
            "qwen_image_2.1_vae_bf16.safetensors",
            "qwen-image-2.1-uncensored-lora.safetensors",
            "qwen3vl_8b_bf16.safetensors",
        ] {
            assert!(bundle_for(f).is_none(), "{f}");
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
    fn every_qwen_edit_bundle_carries_a_vision_projector() {
        for f in [
            "qwen-image-edit-Q4_K_M.gguf",
            "qwen-image-edit-2509-Q4_K_M.gguf",
            "qwen-image-edit-2511-Q4_K_M.gguf",
            "split/diffusion_models/qwen-image-edit-2511-uncensored-Q6_K.gguf",
        ] {
            let b = bundle_for(f).unwrap();
            assert!(b.components.iter().any(|c| c.role == "llm_vision"), "{f}");
        }
    }

    #[test]
    fn companions_require_the_repo_and_entire_relative_path() {
        let c = FLUX2_VAE;
        assert!(is_bundle_companion(&format!("{}/{}", c.repo, c.file)));
        assert!(!is_bundle_companion(c.file));
        assert!(!is_bundle_companion(&format!("other/repo/{}", c.file)));
        assert!(!is_bundle_companion(&format!(
            "{}/elsewhere/{}",
            c.repo, c.file
        )));
    }

    #[test]
    fn own_projector_checks_the_basename_in_an_mmproj_directory() {
        let main = "mmproj/qwen-image-edit-2511-Q4_K_M.gguf";
        let encoder = "mmproj/Qwen2.5-VL-7B-Instruct-Q4_K_M.gguf";
        let projector = "mmproj/Qwen2.5-VL-7B-Instruct-mmproj-f16.gguf";

        assert_eq!(select_own_projector(main, [encoder]), None);
        assert_eq!(
            select_own_projector(main, [encoder, projector]),
            Some(projector)
        );
    }

    #[test]
    fn own_projector_requires_a_family_token_boundary() {
        let main = "diffusion_models/qwen-image-edit-2511-Q4_K_M.gguf";
        let projector = "text_encoders/Qwen2.5-VL-7B-Instruct-mmproj-f16.gguf";
        for incompatible in [
            "text_encoders/Qwen2.5-VL2-mmproj-f16.gguf",
            "text_encoders/Qwen2.5-VLNext-mmproj-f16.gguf",
        ] {
            assert_eq!(select_own_projector(main, [incompatible]), None);
            assert_eq!(
                select_own_projector(main, [incompatible, projector]),
                Some(projector)
            );
        }
        assert_eq!(select_own_projector(main, [projector]), Some(projector));
    }

    #[test]
    fn a_repos_own_projector_wins_over_the_curated_one() {
        let dir = tempfile::tempdir().unwrap();
        let lib = dir.path().to_path_buf();
        let root = lib.join("o/qwen-image-edit-uncensored-GGUF/split");
        std::fs::create_dir_all(root.join("diffusion_models")).unwrap();
        std::fs::create_dir_all(root.join("text_encoders")).unwrap();
        let main = root.join("diffusion_models/qwen-image-edit-2511-uncensored-Q4_K_M.gguf");
        std::fs::write(&main, b"gguf").unwrap();
        let libs = vec![lib.clone()];
        let projector = |p: &ComponentPlan| p.role == "llm_vision";
        let plan = plan_for(
            &main.file_name().unwrap().to_string_lossy(),
            &libs,
            Some(&main),
        )
        .unwrap();
        assert!(plan
            .components
            .iter()
            .find(|c| projector(c))
            .unwrap()
            .path
            .is_none());
        let own = root.join("text_encoders/Qwen2.5-VL-7B-Instruct-mmproj-f16.gguf");
        std::fs::write(&own, b"x").unwrap();
        std::fs::write(
            root.join("text_encoders/Qwen2.5-VL-7B-Instruct-q4_0.gguf"),
            b"x",
        )
        .unwrap();
        let plan = plan_for(
            &main.file_name().unwrap().to_string_lossy(),
            &libs,
            Some(&main),
        )
        .unwrap();
        let found = plan.components.iter().find(|c| projector(c)).unwrap();
        assert_eq!(found.path.as_deref().map(Path::new), Some(own.as_path()));
        // The text encoder is never taken for a projector.
        assert!(is_projector_file("a/b-mmproj-f16.gguf"));
        assert!(!is_projector_file("a/Qwen2.5-VL-7B-Instruct-q4_0.gguf"));
    }

    #[test]
    fn the_last_companion_to_arrive_completes_the_model() {
        // A text encoder or projector GGUF has no image-model name but finishes a bundle.
        for c in [QWEN3_4B, QWEN25_VL_7B_MMPROJ, FLUX2_VAE] {
            let f = format!("{}/{}", c.repo, c.file);
            assert!(concerns_image_models(&f), "{f}");
        }
        assert!(concerns_image_models(
            "split/text_encoders/Qwen2.5-VL-7B-Instruct-mmproj-f16.gguf"
        ));
        assert!(!concerns_image_models("Llama-3.2-1B-Q4_K_M.gguf"));
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
        assert_eq!(
            variant_quant("sd_xl_offset_example-lora_1.0.safetensors"),
            "component"
        );
        assert_eq!(
            variant_quant("pytorch_lora_weights.safetensors"),
            "component"
        );
        assert_eq!(variant_quant("flora-xl-fp16.safetensors"), "FP16");
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
                let variant = DownloadManager::companion_variant(c.repo, c.file, None)
                    .await
                    .unwrap_or_else(|e| panic!("{}/{}: {e}", c.repo, c.file));
                let found = &variant.shards[0];
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
