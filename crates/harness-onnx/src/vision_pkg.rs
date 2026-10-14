//! The repo of a Hugging Face ONNX vision model (transformers.js / Optimum exports):
//!
//! ```text
//! <repo>/config.json               architectures, id2label
//! <repo>/preprocessor_config.json  resize, crop, rescale, normalize
//! <repo>/onnx/model.onnx           (or model_quantized.onnx, model_fp16.onnx...)
//! ```
//!
//! What the image processor does before the graph and which head the graph has are read here;
//! decoding and resizing the image happen in `vision` (feature `engine`). Pure.

use crate::generation::GenConfig;
use crate::package::labels_from_config;
use serde_json::Value;
use std::path::{Path, PathBuf};

/// The task of a vision head.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VisionTask {
    /// `*ForImageClassification`: labels with scores.
    Classify,
    /// `*ForObjectDetection` (DETR, YOLOS, Conditional / Deformable DETR): boxes with labels.
    Detect,
    /// `*ForSemanticSegmentation` (SegFormer, BEiT, MobileViT, DPT...): a mask per label.
    Segment,
    /// `*ForDepthEstimation` (DPT, Depth Anything, GLPN, ZoeDepth): a depth map.
    Depth,
    /// A dual encoder (`CLIPModel`, `SiglipModel`) exported as one graph: the image scored
    /// against labels the request names.
    ZeroShot,
    /// An open-vocabulary detector (`OwlViTForObjectDetection`, `Owlv2ForObjectDetection`):
    /// boxes for the labels the request names.
    ZeroShotDetect,
    /// A bare image backbone (`ViTModel`, `Dinov2Model`, `ConvNextModel`...): its hidden
    /// states (or pooled output) as the image's features.
    Features,
    /// An image captioner (`VisionEncoderDecoderModel`: ViT + GPT-2...) exported as an encoder
    /// and a text decoder: the caption generated token by token.
    Caption,
}

/// `model_type`s of image backbones whose bare `*Model` export is an image feature extractor.
const IMAGE_BACKBONES: &[&str] = &[
    "vit",
    "vit_mae",
    "vit_msn",
    "deit",
    "beit",
    "data2vec-vision",
    "dinov2",
    "dinov2_with_registers",
    "ijepa",
    "convnext",
    "convnextv2",
    "swin",
    "swinv2",
    "mobilevit",
    "mobilevitv2",
    "efficientnet",
    "resnet",
    "regnet",
    "levit",
    "pvt",
    "pvt_v2",
    "clip_vision_model",
    "siglip_vision_model",
];

impl VisionTask {
    /// The Hugging Face pipeline task.
    pub fn pipeline_task(self) -> &'static str {
        match self {
            VisionTask::Classify => "image-classification",
            VisionTask::Detect => "object-detection",
            VisionTask::Segment => "image-segmentation",
            VisionTask::Depth => "depth-estimation",
            VisionTask::ZeroShot => "zero-shot-image-classification",
            VisionTask::ZeroShotDetect => "zero-shot-object-detection",
            VisionTask::Features => "image-feature-extraction",
            VisionTask::Caption => "image-to-text",
        }
    }
}

/// Pixel filter of a resize (`resample` of the processor: PIL's numbering).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Resample {
    Nearest,
    Bilinear,
    Bicubic,
}

impl Resample {
    fn from_pil(n: Option<u64>) -> Self {
        match n {
            Some(0) => Resample::Nearest,
            Some(3) | Some(1) => Resample::Bicubic,
            _ => Resample::Bilinear,
        }
    }
}

/// The target size of the resize step.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Size {
    /// An exact size.
    Exact { width: u32, height: u32 },
    /// The short edge becomes `short`, the long edge at most `long` (aspect kept).
    ShortestEdge { short: u32, long: Option<u32> },
    /// DPT / Depth Anything: scaled so that both edges are at least the target, keeping the
    /// aspect ratio, then rounded to a multiple of `multiple`.
    KeepAspect {
        width: u32,
        height: u32,
        multiple: u32,
    },
    /// GLPN: both edges rounded down to a multiple of `divisor`.
    Divisor(u32),
}

/// What the image processor does, in order: resize, center crop, rescale, normalize.
#[derive(Debug, Clone, PartialEq)]
pub struct Preprocess {
    pub resize: Option<Size>,
    pub resample: Resample,
    /// Center crop to this size after the resize.
    pub crop: Option<(u32, u32)>,
    /// Multiply 0..255 values by this (1/255 by default).
    pub rescale: Option<f32>,
    pub mean: [f32; 3],
    pub std: [f32; 3],
    pub normalize: bool,
    /// The graph takes BGR (some ports of Caffe-era models).
    pub bgr: bool,
    /// Pad the image to a square (bottom and right, grey) before the resize, as OWLv2 does;
    /// boxes are then relative to the padded square.
    pub pad_square: bool,
}

/// The text side of a zero-shot image model: its tokenizer and how labels are encoded.
#[derive(Debug, Clone, PartialEq)]
pub struct TextSide {
    pub tokenizer_path: PathBuf,
    /// Longest label in tokens (`text_config.max_position_embeddings`; 77 for CLIP).
    pub max_length: usize,
    /// Pad every label to `max_length` (SigLIP was trained that way; CLIP pads to the longest).
    pub pad_to_max: bool,
    /// Lower-case the label sentences and drop their punctuation before tokenizing, as
    /// transformers' `SiglipTokenizer` does (its `tokenizer.json` does not).
    pub canonicalize: bool,
    /// Each label is scored on its own (SigLIP's sigmoid loss); CLIP's scores are a softmax
    /// across the labels.
    pub sigmoid: bool,
    /// The `pad_token` of `tokenizer_config.json`.
    pub pad_token: Option<String>,
}

#[derive(Debug, Clone)]
pub struct VisionPackage {
    pub onnx_path: PathBuf,
    pub root: PathBuf,
    pub task: VisionTask,
    pub labels: Vec<String>,
    /// Labels are independent (`problem_type: multi_label_classification`): sigmoid.
    pub multi_label: bool,
    /// Feature extraction: the model's pooled output is its first (CLS) hidden state, so
    /// `pool` works on an export without `pooler_output` (DINOv2).
    pub cls_pool: bool,
    pub preprocess: Preprocess,
    /// The tokenizer of a zero-shot model (`None` for the other tasks).
    pub text: Option<TextSide>,
    /// The text decoder of a captioner (`None` for the other tasks).
    pub caption: Option<CaptionSide>,
    /// Display name: the repo folder.
    pub name: String,
    pub notes: Vec<String>,
}

fn read_json(path: &Path) -> Option<Value> {
    serde_json::from_slice(&std::fs::read(path).ok()?).ok()
}

/// The folder that holds `preprocessor_config.json` for a model file (the file's folder or
/// its parent, as in `onnx/model.onnx`).
pub fn find_root(onnx: &Path) -> Option<PathBuf> {
    onnx.ancestors()
        .skip(1)
        .take(2)
        .find(|d| d.join("preprocessor_config.json").is_file())
        .map(Path::to_path_buf)
}

/// The vision head of a `config.json`, `None` for any other model.
pub fn task_from_config(config: &Value) -> Option<VisionTask> {
    let heads: Vec<&str> = config
        .get("architectures")
        .and_then(Value::as_array)
        .map(|a| a.iter().filter_map(Value::as_str).collect())
        .unwrap_or_default();
    let has = |suffix: &str| heads.iter().any(|n| n.ends_with(suffix));
    if heads
        .iter()
        .any(|n| matches!(*n, "OwlViTForObjectDetection" | "Owlv2ForObjectDetection"))
    {
        return Some(VisionTask::ZeroShotDetect);
    }
    if has("ForImageClassification") || has("ForImageClassificationWithTeacher") {
        Some(VisionTask::Classify)
    } else if has("ForObjectDetection") {
        Some(VisionTask::Detect)
    } else if has("ForSemanticSegmentation") {
        Some(VisionTask::Segment)
    } else if has("ForDepthEstimation") {
        Some(VisionTask::Depth)
    } else if heads
        .iter()
        .any(|n| matches!(*n, "CLIPModel" | "SiglipModel"))
    {
        Some(VisionTask::ZeroShot)
    } else if heads.contains(&"VisionEncoderDecoderModel") {
        Some(VisionTask::Caption)
    } else if heads.iter().any(|n| n.ends_with("Model"))
        && config
            .get("model_type")
            .and_then(Value::as_str)
            .is_some_and(|t| IMAGE_BACKBONES.contains(&t))
    {
        Some(VisionTask::Features)
    } else {
        None
    }
}

fn u32_of(v: Option<&Value>) -> Option<u32> {
    v.and_then(Value::as_u64)
        .and_then(|n| u32::try_from(n).ok())
        .filter(|n| *n > 0)
}

/// `size` / `crop_size`: a number, `{height, width}`, `{shortest_edge[, longest_edge]}`.
fn size_of(v: Option<&Value>) -> Option<Size> {
    let v = v?;
    if let Some(n) = u32_of(Some(v)) {
        return Some(Size::Exact {
            width: n,
            height: n,
        });
    }
    if let (Some(h), Some(w)) = (u32_of(v.get("height")), u32_of(v.get("width"))) {
        return Some(Size::Exact {
            width: w,
            height: h,
        });
    }
    u32_of(v.get("shortest_edge")).map(|short| Size::ShortestEdge {
        short,
        long: u32_of(v.get("longest_edge")),
    })
}

fn triple(v: Option<&Value>, default: [f32; 3]) -> [f32; 3] {
    match v {
        Some(Value::Array(a)) if a.len() == 3 => {
            let mut out = default;
            for (o, x) in out.iter_mut().zip(a) {
                if let Some(f) = x.as_f64() {
                    *o = f as f32;
                }
            }
            out
        }
        Some(x) => x.as_f64().map(|f| [f as f32; 3]).unwrap_or(default),
        None => default,
    }
}

/// The preprocessing of a `preprocessor_config.json` (defaults of transformers' image
/// processors where a field is missing).
pub fn parse_preprocess(v: &Value) -> Preprocess {
    let flag = |k: &str, default: bool| v.get(k).and_then(Value::as_bool).unwrap_or(default);
    let resize = if !flag("do_resize", true) {
        None
    } else if flag("keep_aspect_ratio", false) {
        match size_of(v.get("size")) {
            Some(Size::Exact { width, height }) => Some(Size::KeepAspect {
                width,
                height,
                multiple: u32_of(v.get("ensure_multiple_of")).unwrap_or(1),
            }),
            other => other,
        }
    } else if let Some(d) = u32_of(v.get("size_divisor")).filter(|_| v.get("size").is_none()) {
        Some(Size::Divisor(d))
    } else {
        size_of(v.get("size"))
    };
    let crop = if flag("do_center_crop", false) {
        match size_of(v.get("crop_size")) {
            Some(Size::Exact { width, height }) => Some((width, height)),
            Some(Size::ShortestEdge { short, .. }) => Some((short, short)),
            _ => None,
        }
    } else {
        None
    };
    let rescale = flag("do_rescale", true).then(|| {
        v.get("rescale_factor")
            .and_then(Value::as_f64)
            .map(|f| f as f32)
            .unwrap_or(1.0 / 255.0)
    });
    Preprocess {
        resize,
        resample: Resample::from_pil(v.get("resample").and_then(Value::as_u64)),
        crop,
        rescale,
        mean: triple(v.get("image_mean"), [0.5; 3]),
        std: triple(v.get("image_std"), [0.5; 3]),
        normalize: flag("do_normalize", true),
        bgr: flag("do_flip_channel_order", false),
        pad_square: flag("do_pad", false)
            && v.get("image_processor_type")
                .and_then(Value::as_str)
                .is_some_and(|t| t.starts_with("Owlv2")),
    }
}

impl VisionPackage {
    /// Reads the repo around `onnx_path`; `None` when it is not a vision model this engine
    /// serves.
    pub fn discover(onnx_path: &Path) -> Option<Self> {
        if !onnx_path
            .extension()
            .is_some_and(|e| e.eq_ignore_ascii_case("onnx"))
        {
            return None;
        }
        let root = find_root(onnx_path)?;
        let config = read_json(&root.join("config.json"))?;
        let task = task_from_config(&config)?;
        let processor = read_json(&root.join("preprocessor_config.json"))?;
        let preprocess = parse_preprocess(&processor);
        // A captioner is found by its encoder graph; the decoder graphs belong to it.
        let caption = if task == VisionTask::Caption {
            Some(caption_side(&root, onnx_path)?)
        } else {
            None
        };
        let text = if matches!(task, VisionTask::ZeroShot | VisionTask::ZeroShotDetect) {
            // transformers.js also ships the two towers alone (`text_model`, `vision_model`);
            // only the joint graph scores an image against labels.
            let stem = onnx_path
                .file_stem()?
                .to_string_lossy()
                .to_ascii_lowercase();
            if stem.starts_with("text_model") || stem.starts_with("vision_model") {
                return None;
            }
            Some(text_side(&root, &config)?)
        } else {
            None
        };
        let mut notes = vec![format!(
            "task {} (config.json architectures)",
            task.pipeline_task()
        )];
        notes.push(format!(
            "resize {:?} ({:?}), crop {:?}, rescale {:?}, normalize {} mean {:?} std {:?}",
            preprocess.resize,
            preprocess.resample,
            preprocess.crop,
            preprocess.rescale,
            preprocess.normalize,
            preprocess.mean,
            preprocess.std
        ));
        let name = root
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| "onnx-model".to_string());
        Some(VisionPackage {
            onnx_path: onnx_path.to_path_buf(),
            root,
            task,
            labels: labels_from_config(&config),
            multi_label: config.get("problem_type").and_then(Value::as_str)
                == Some("multi_label_classification"),
            cls_pool: matches!(
                config.get("model_type").and_then(Value::as_str),
                Some("dinov2" | "dinov2_with_registers")
            ),
            preprocess,
            text,
            caption,
            name,
            notes,
        })
    }
}

/// The text decoder of an image captioner and how it generates.
#[derive(Debug, Clone, PartialEq)]
pub struct CaptionSide {
    /// `decoder_model_merged*.onnx` (one graph with and without the KV cache) or
    /// `decoder_model*.onnx` (no cache: the whole sequence every step).
    pub decoder_path: PathBuf,
    pub tokenizer_path: PathBuf,
    pub gen: GenConfig,
}

/// The decoder, tokenizer and generation settings of a captioner's encoder graph; `None` for
/// a decoder graph itself or a repo without them.
fn caption_side(root: &Path, encoder: &Path) -> Option<CaptionSide> {
    let decoder_path = crate::generation::decoder_beside(encoder)?;
    let tokenizer_path = root.join("tokenizer.json");
    if !tokenizer_path.is_file() {
        return None;
    }
    Some(CaptionSide {
        decoder_path,
        tokenizer_path,
        gen: GenConfig::read(root, None)?,
    })
}

/// The tokenizer side of a zero-shot model; `None` without a `tokenizer.json`.
fn text_side(root: &Path, config: &Value) -> Option<TextSide> {
    let tokenizer_path = root.join("tokenizer.json");
    if !tokenizer_path.is_file() {
        return None;
    }
    let model_type = config.get("model_type").and_then(Value::as_str);
    let siglip = model_type == Some("siglip");
    // OWL-ViT queries are padded to 16 tokens and scored one by one.
    let owl = matches!(model_type, Some("owlvit" | "owlv2"));
    let max_length = config
        .pointer("/text_config/max_position_embeddings")
        .and_then(Value::as_u64)
        .map(|n| n as usize)
        .unwrap_or(if siglip {
            64
        } else if owl {
            16
        } else {
            77
        });
    // `pad_token` is a string or an AddedToken object.
    let pad_token = read_json(&root.join("tokenizer_config.json"))
        .and_then(|t| t.get("pad_token").cloned())
        .and_then(|p| match p {
            Value::String(s) => Some(s),
            Value::Object(o) => o.get("content").and_then(Value::as_str).map(str::to_string),
            _ => None,
        });
    Some(TextSide {
        tokenizer_path,
        max_length,
        pad_to_max: siglip || owl,
        canonicalize: siglip,
        sigmoid: siglip || owl,
        pad_token,
    })
}

/// The pipeline task of a model file for the library: a text task of an encoder package or a
/// vision task, `None` for embedders, rerankers and any other graph.
pub fn pipeline_task_of(onnx_path: &Path) -> Option<&'static str> {
    if let Ok(p) = crate::package::ModelPackage::discover(onnx_path) {
        return p.pipeline_task();
    }
    VisionPackage::discover(onnx_path).map(|p| p.task.pipeline_task())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::fs;

    #[test]
    fn heads_map_to_tasks() {
        // Open-vocabulary detectors are not the closed-set detection head.
        assert_eq!(
            task_from_config(&json!({"architectures": ["Owlv2ForObjectDetection"]})),
            Some(VisionTask::ZeroShotDetect)
        );
        assert_eq!(
            task_from_config(&json!({"architectures": ["OwlViTForObjectDetection"]})),
            Some(VisionTask::ZeroShotDetect)
        );
        let t = |a: &str| task_from_config(&json!({"architectures": [a]}));
        assert_eq!(t("ViTForImageClassification"), Some(VisionTask::Classify));
        assert_eq!(
            t("DeiTForImageClassificationWithTeacher"),
            Some(VisionTask::Classify)
        );
        assert_eq!(t("DetrForObjectDetection"), Some(VisionTask::Detect));
        assert_eq!(
            t("SegformerForSemanticSegmentation"),
            Some(VisionTask::Segment)
        );
        assert_eq!(
            t("DepthAnythingForDepthEstimation"),
            Some(VisionTask::Depth)
        );
        assert_eq!(t("CLIPModel"), Some(VisionTask::ZeroShot));
        assert_eq!(t("SiglipModel"), Some(VisionTask::ZeroShot));
        assert_eq!(t("CLIPVisionModelWithProjection"), None);
        assert_eq!(t("WhisperForConditionalGeneration"), None);
        assert_eq!(task_from_config(&json!({})), None);
        // A bare backbone is a feature extractor only when its model type is an image one.
        let bare =
            |a: &str, t: &str| task_from_config(&json!({"architectures": [a], "model_type": t}));
        assert_eq!(bare("Dinov2Model", "dinov2"), Some(VisionTask::Features));
        assert_eq!(bare("ViTModel", "vit"), Some(VisionTask::Features));
        assert_eq!(bare("BertModel", "bert"), None);
        assert_eq!(bare("Wav2Vec2Model", "wav2vec2"), None);
    }

    #[test]
    fn processors_of_common_models() {
        // OWLv2 pads to a square; DETR's do_pad (batch padding) is no square padding.
        let owlv2 = parse_preprocess(&json!({
            "do_pad": true, "image_processor_type": "Owlv2ImageProcessor",
            "size": {"height": 960, "width": 960}
        }));
        assert!(owlv2.pad_square);
        let detr = parse_preprocess(
            &json!({"do_pad": true, "image_processor_type": "DetrImageProcessor"}),
        );
        assert!(!detr.pad_square);
        // google/vit-base-patch16-224
        let vit = parse_preprocess(&json!({
            "do_normalize": true, "do_resize": true, "image_mean": [0.5, 0.5, 0.5],
            "image_std": [0.5, 0.5, 0.5], "resample": 2, "size": 224
        }));
        assert_eq!(
            vit.resize,
            Some(Size::Exact {
                width: 224,
                height: 224
            })
        );
        assert_eq!(vit.resample, Resample::Bilinear);
        assert_eq!(vit.rescale, Some(1.0 / 255.0));
        // facebook/detr-resnet-50
        let detr = parse_preprocess(&json!({
            "do_normalize": true, "do_resize": true, "image_mean": [0.485, 0.456, 0.406],
            "image_std": [0.229, 0.224, 0.225], "size": {"shortest_edge": 800, "longest_edge": 1333}
        }));
        assert_eq!(
            detr.resize,
            Some(Size::ShortestEdge {
                short: 800,
                long: Some(1333)
            })
        );
        assert_eq!(detr.mean, [0.485, 0.456, 0.406]);
        // LiheYoung/depth-anything-small-hf
        let da = parse_preprocess(&json!({
            "do_resize": true, "keep_aspect_ratio": true, "ensure_multiple_of": 14,
            "resample": 3, "size": {"height": 518, "width": 518}, "do_rescale": true,
            "rescale_factor": 0.00392156862745098, "image_mean": [0.485, 0.456, 0.406],
            "image_std": [0.229, 0.224, 0.225]
        }));
        assert_eq!(
            da.resize,
            Some(Size::KeepAspect {
                width: 518,
                height: 518,
                multiple: 14
            })
        );
        assert_eq!(da.resample, Resample::Bicubic);
        // ConvNeXt-style crop
        let crop = parse_preprocess(&json!({
            "size": {"shortest_edge": 256}, "do_center_crop": true, "crop_size": {"height": 224, "width": 224},
            "do_rescale": false
        }));
        assert_eq!(crop.crop, Some((224, 224)));
        assert_eq!(crop.rescale, None);
        // GLPN
        let glpn = parse_preprocess(&json!({"do_resize": true, "size_divisor": 32}));
        assert_eq!(glpn.resize, Some(Size::Divisor(32)));
    }

    #[test]
    fn discovers_a_transformers_js_repo() {
        let dir = tempfile::tempdir().unwrap();
        let repo = dir.path().join("Xenova").join("vit-base-patch16-224");
        fs::create_dir_all(repo.join("onnx")).unwrap();
        fs::write(
            repo.join("config.json"),
            json!({"architectures": ["ViTForImageClassification"], "id2label": {"0": "tench", "1": "goldfish"}})
                .to_string(),
        )
        .unwrap();
        fs::write(
            repo.join("preprocessor_config.json"),
            json!({"size": 224}).to_string(),
        )
        .unwrap();
        let model = repo.join("onnx").join("model_quantized.onnx");
        fs::write(&model, b"onnx").unwrap();
        let p = VisionPackage::discover(&model).unwrap();
        assert_eq!(p.task, VisionTask::Classify);
        assert_eq!(p.labels, vec!["tench", "goldfish"]);
        assert_eq!(p.name, "vit-base-patch16-224");
        assert_eq!(pipeline_task_of(&model), Some("image-classification"));
        // Without a processor config it is not a vision package.
        fs::remove_file(repo.join("preprocessor_config.json")).unwrap();
        assert!(VisionPackage::discover(&model).is_none());
    }

    #[test]
    fn discovers_a_captioner_by_its_encoder() {
        let dir = tempfile::tempdir().unwrap();
        let repo = dir.path().join("Xenova").join("vit-gpt2-image-captioning");
        fs::create_dir_all(repo.join("onnx")).unwrap();
        fs::write(
            repo.join("config.json"),
            json!({"architectures": ["VisionEncoderDecoderModel"], "decoder_start_token_id": 50256,
                   "eos_token_id": 50256})
            .to_string(),
        )
        .unwrap();
        fs::write(
            repo.join("preprocessor_config.json"),
            json!({"size": {"height": 224, "width": 224}}).to_string(),
        )
        .unwrap();
        fs::write(repo.join("tokenizer.json"), "{}").unwrap();
        let onnx = repo.join("onnx");
        for f in [
            "encoder_model_quantized.onnx",
            "decoder_model_merged.onnx",
            "decoder_model_merged_quantized.onnx",
        ] {
            fs::write(onnx.join(f), b"onnx").unwrap();
        }
        let encoder = onnx.join("encoder_model_quantized.onnx");
        let p = VisionPackage::discover(&encoder).unwrap();
        assert_eq!(p.task, VisionTask::Caption);
        let side = p.caption.unwrap();
        // The decoder of the same precision; 19 new tokens by default (transformers' 20 in all).
        assert_eq!(
            side.decoder_path,
            onnx.join("decoder_model_merged_quantized.onnx")
        );
        assert_eq!(
            (side.gen.start_id, side.gen.eos_ids.clone()),
            (50256, vec![50256])
        );
        assert_eq!(side.gen.length_of(None, 1), 20);
        assert_eq!(pipeline_task_of(&encoder), Some("image-to-text"));
        // The decoder graphs belong to the encoder; they are no models of their own.
        assert!(VisionPackage::discover(&onnx.join("decoder_model_merged.onnx")).is_none());
        // Without a tokenizer there is no caption to decode.
        fs::rename(repo.join("tokenizer.json"), repo.join("tokenizer.bak")).unwrap();
        assert!(VisionPackage::discover(&encoder).is_none());
        fs::rename(repo.join("tokenizer.bak"), repo.join("tokenizer.json")).unwrap();
        // The repo has a tokenizer, but it is no text encoder.
        assert!(matches!(
            crate::package::ModelPackage::discover(&encoder),
            Err(crate::package::PackageError::DualEncoder(_))
        ));
    }

    #[test]
    fn discovers_a_clip_repo_by_its_joint_graph() {
        let dir = tempfile::tempdir().unwrap();
        let repo = dir.path().join("Xenova").join("clip-vit-base-patch32");
        fs::create_dir_all(repo.join("onnx")).unwrap();
        fs::write(
            repo.join("config.json"),
            json!({"architectures": ["CLIPModel"], "model_type": "clip",
                   "text_config": {"max_position_embeddings": 77}})
            .to_string(),
        )
        .unwrap();
        fs::write(
            repo.join("preprocessor_config.json"),
            json!({"size": {"shortest_edge": 224}, "do_center_crop": true,
                   "crop_size": {"height": 224, "width": 224}, "resample": 3})
            .to_string(),
        )
        .unwrap();
        fs::write(
            repo.join("tokenizer_config.json"),
            json!({"pad_token": "<|endoftext|>"}).to_string(),
        )
        .unwrap();
        fs::write(repo.join("tokenizer.json"), "{}").unwrap();
        let model = repo.join("onnx").join("model_quantized.onnx");
        fs::write(&model, b"onnx").unwrap();
        let p = VisionPackage::discover(&model).unwrap();
        assert_eq!(p.task, VisionTask::ZeroShot);
        assert_eq!(
            p.text,
            Some(TextSide {
                tokenizer_path: repo.join("tokenizer.json"),
                max_length: 77,
                pad_to_max: false,
                canonicalize: false,
                sigmoid: false,
                pad_token: Some("<|endoftext|>".into()),
            })
        );
        assert_eq!(
            pipeline_task_of(&model),
            Some("zero-shot-image-classification")
        );
        // One tower alone cannot score an image against labels.
        let tower = repo.join("onnx").join("vision_model_quantized.onnx");
        fs::write(&tower, b"onnx").unwrap();
        assert!(VisionPackage::discover(&tower).is_none());
        // Nor can the joint graph without its tokenizer.
        fs::remove_file(repo.join("tokenizer.json")).unwrap();
        assert!(VisionPackage::discover(&model).is_none());
    }
}
