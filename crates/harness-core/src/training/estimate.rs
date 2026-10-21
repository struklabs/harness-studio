//! Memory and time estimate of a training job, and which tuning methods this machine can run.
//!
//! The numbers are planning estimates (weights in the training dtype, the LoRA adapter with
//! its gradients and Adam state, activations without checkpointing, the logits, runtime
//! overhead; time from a rough throughput per device). They are meant to say "fits easily",
//! "tight" or "will not fit", not to be exact.

use super::{Device, Method};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;
use std::path::Path;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct Estimate {
    /// Parameters of the base model.
    pub params: u64,
    /// Memory the run needs on the training device.
    pub memory_bytes: u64,
    /// Memory free on that device now (RAM for CPU and Apple Silicon, VRAM for CUDA).
    pub available_bytes: u64,
    pub fits: bool,
    /// Training time of all steps, in seconds (plus a fixed start-up share).
    pub seconds: u64,
    pub seconds_per_step: f64,
    pub tokens_per_step: u64,
}

/// What the estimate reads from a model folder.
#[derive(Debug, Clone, Copy, Default)]
pub struct ModelShape {
    pub params: u64,
    pub hidden: u64,
    pub layers: u64,
    pub intermediate: u64,
    pub vocab: u64,
}

pub fn model_shape(dir: &Path) -> Result<ModelShape, String> {
    let cfg: Value = std::fs::read_to_string(dir.join("config.json"))
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .ok_or_else(|| {
            String::from(
                crate::error_key::err("training.model_config_missing").with("path", dir.display()),
            )
        })?;
    let n = |k: &str| cfg.get(k).and_then(Value::as_u64).unwrap_or(0);
    let hidden = n("hidden_size");
    let layers = n("num_hidden_layers");
    let intermediate = match n("intermediate_size") {
        0 => hidden * 4,
        v => v,
    };
    let vocab = n("vocab_size");
    let file_bytes: u64 = crate::engine::python_hf::weight_files(dir)
        .iter()
        .filter_map(|p| std::fs::metadata(p).ok())
        .map(|m| m.len())
        .sum();
    let dtype = cfg
        .get("torch_dtype")
        .or_else(|| cfg.get("dtype"))
        .and_then(Value::as_str)
        .unwrap_or("float32");
    let bytes_per_param = if dtype.contains("32") { 4 } else { 2 };
    let from_files = file_bytes / bytes_per_param;
    // Without weight files (a hub stub) fall back to the architecture.
    let from_arch = layers * (4 * hidden * hidden + 3 * hidden * intermediate) + vocab * hidden;
    let params = if from_files > 0 {
        from_files
    } else {
        from_arch
    };
    if params == 0 || hidden == 0 || layers == 0 {
        return Err(crate::error_key::err("training.model_unsupported")
            .with("path", dir.display())
            .into());
    }
    Ok(ModelShape {
        params,
        hidden,
        layers,
        intermediate,
        vocab,
    })
}

/// Free memory of the device the job trains on.
pub fn available_bytes(device: Device) -> u64 {
    match device {
        Device::Cuda => crate::jobs::available_memory().0,
        Device::Cpu | Device::Mps => crate::hardware::detect_hardware().available_ram_bytes,
    }
}

fn cpu_cores() -> u64 {
    crate::hardware::detect_hardware().cpu_cores.max(1) as u64
}

/// The estimate for one configuration.
pub fn estimate(
    shape: &ModelShape,
    method: Method,
    device: Device,
    rank: u32,
    sequence_length: u32,
    steps: u32,
    avg_row_tokens: u64,
) -> Estimate {
    let params = shape.params as f64;
    // CPU trains in fp32; CUDA and MPS in fp16; QLoRA holds the base weights in 4 bits.
    let (weight_bytes, act_bytes) = match (method, device) {
        (_, Device::Cpu) => (4.0, 4.0),
        (Method::Qlora, _) => (0.56, 2.0),
        _ => (2.0, 2.0),
    };
    let seq = u64::from(sequence_length).min(avg_row_tokens.max(8)) as f64;
    let seq_max = f64::from(sequence_length);
    let lora_params = f64::from(rank)
        * shape.layers as f64
        * (11.0 * shape.hidden as f64 + 3.0 * shape.intermediate as f64);
    // fp32 adapter + gradient + two Adam moments.
    let adapter = lora_params * 16.0;
    let activations = shape.layers as f64 * seq_max * shape.hidden as f64 * act_bytes * 18.0;
    let logits = seq_max * shape.vocab as f64 * 4.0 * 3.0;
    let overhead = match device {
        Device::Cpu => 512.0 * 1024.0 * 1024.0,
        _ => 1024.0 * 1024.0 * 1024.0,
    };
    let memory = params * weight_bytes + adapter + activations + logits + overhead;

    // Forward 2N + backward through the activations 2N flops per token (LoRA has no weight
    // gradients of the base model).
    let tokens_per_step = seq.max(1.0);
    let flops = 4.0 * params * tokens_per_step;
    let throughput = match device {
        Device::Cpu => cpu_cores() as f64 * 10e9,
        Device::Cuda => 8e12,
        Device::Mps => 2.5e12,
    };
    let slowdown = if method == Method::Qlora { 1.5 } else { 1.0 };
    // Evaluation of the validation rows adds a share, the optimizer step a little more.
    let per_step = flops / throughput * slowdown * 1.2 + 0.05;
    let available = available_bytes(device);
    Estimate {
        params: shape.params,
        memory_bytes: memory as u64,
        available_bytes: available,
        fits: available == 0 || (memory as u64) < available,
        seconds: (per_step * f64::from(steps)) as u64 + 20,
        seconds_per_step: per_step,
        tokens_per_step: tokens_per_step as u64,
    }
}

// --- what this machine can run ----------------------------------------------------------------

/// What the training worker found in the environment (`training_worker.py --probe`).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct Probe {
    pub devices: Vec<String>,
    pub qlora: bool,
    pub qlora_reason: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[cfg_attr(feature = "openapi", schema(as = TrainingSupport))]
pub enum Support {
    Supported,
    /// Works with a stated limit, or not verified on this machine yet.
    Limited,
    Unsupported,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct MethodSupport {
    /// `lora`, `qlora`, `sft`, `dpo`, `full`.
    pub method: String,
    pub support: Support,
    /// i18n key (`training.compat_*`) with its values.
    pub reason: String,
    pub params: BTreeMap<String, String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct Compatibility {
    pub os: String,
    /// Devices a job can use here (`cpu`, `cuda`, `mps`).
    pub devices: Vec<String>,
    /// The environment answered a probe (else the CUDA and QLoRA lines are inferred).
    pub probed: bool,
    pub methods: Vec<MethodSupport>,
}

/// Devices the hardware offers, before the environment confirms them.
pub fn hardware_devices() -> Vec<String> {
    let mut devices = vec!["cpu".to_string()];
    if crate::pyenv::envs::cuda_present() {
        devices.push("cuda".into());
    }
    if cfg!(all(target_os = "macos", target_arch = "aarch64")) {
        devices.push("mps".into());
    }
    devices
}

fn row(method: &str, support: Support, reason: &str, params: &[(&str, String)]) -> MethodSupport {
    MethodSupport {
        method: method.into(),
        support,
        reason: reason.into(),
        params: params
            .iter()
            .map(|(k, v)| (k.to_string(), v.clone()))
            .collect(),
    }
}

fn billions(params: f64) -> String {
    format!("{:.1}", params / 1e9)
}

/// The compatibility table of this machine. `probe` is the environment's own answer when the
/// Python environment is installed.
pub fn compatibility(probe: Option<&Probe>) -> Compatibility {
    let devices = probe
        .map(|p| p.devices.clone())
        .unwrap_or_else(hardware_devices);
    let cuda = devices.iter().any(|d| d == "cuda");
    let mac = cfg!(target_os = "macos");
    let accel = if cuda {
        Device::Cuda
    } else if devices.iter().any(|d| d == "mps") {
        Device::Mps
    } else {
        Device::Cpu
    };
    let free = available_bytes(accel) as f64;
    // LoRA: weights in the training dtype plus about half again for activations and adapter.
    let lora_dtype = if accel == Device::Cpu { 4.0 } else { 2.0 };
    let lora_max = free * 0.8 / (lora_dtype * 1.5);
    let full_max = free * 0.8 / 16.0;
    let devices_text = devices.join(", ");

    let qlora = match (probe, cuda) {
        (Some(p), _) if p.qlora => row(
            "qlora",
            Support::Supported,
            "training.compat_qlora_ok",
            &[("devices", devices_text.clone())],
        ),
        (Some(p), true) => row(
            "qlora",
            Support::Unsupported,
            "training.compat_qlora_bnb_broken",
            &[("reason", p.qlora_reason.clone())],
        ),
        (None, true) => row(
            "qlora",
            Support::Limited,
            "training.compat_qlora_unverified",
            &[],
        ),
        (_, false) if mac => row(
            "qlora",
            Support::Unsupported,
            "training.compat_qlora_no_cuda_mac",
            &[],
        ),
        (_, false) => row(
            "qlora",
            Support::Unsupported,
            "training.compat_qlora_no_cuda",
            &[],
        ),
    };
    let lora = if accel == Device::Cpu {
        row(
            "lora",
            Support::Limited,
            "training.compat_lora_cpu",
            &[("max_params", billions(lora_max))],
        )
    } else {
        row(
            "lora",
            Support::Supported,
            "training.compat_lora_ok",
            &[
                ("devices", devices_text),
                ("max_params", billions(lora_max)),
            ],
        )
    };
    Compatibility {
        os: std::env::consts::OS.to_string(),
        probed: probe.is_some(),
        devices: devices.clone(),
        methods: vec![
            lora,
            qlora,
            row("sft", Support::Supported, "training.compat_sft_ok", &[]),
            row(
                "dpo",
                Support::Unsupported,
                "training.compat_dpo_planned",
                &[("max_params", billions(lora_max / 2.0))],
            ),
            row(
                "full",
                Support::Unsupported,
                "training.compat_full_not_offered",
                &[("max_params", billions(full_max))],
            ),
        ],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn small() -> ModelShape {
        ModelShape {
            params: 135_000_000,
            hidden: 576,
            layers: 30,
            intermediate: 1536,
            vocab: 49152,
        }
    }

    #[test]
    fn small_model_fits_and_cuda_beats_cpu() {
        let cpu = estimate(&small(), Method::Lora, Device::Cpu, 8, 512, 10, 200);
        assert!(cpu.memory_bytes > 800 * 1024 * 1024 && cpu.memory_bytes < 4 * 1024 * 1024 * 1024);
        assert!(cpu.seconds >= 21);
        let gpu = estimate(&small(), Method::Lora, Device::Cuda, 8, 512, 10, 200);
        assert!(gpu.seconds_per_step < cpu.seconds_per_step);
        let q = estimate(&small(), Method::Qlora, Device::Cuda, 8, 512, 10, 200);
        assert!(q.memory_bytes < gpu.memory_bytes);
    }

    #[test]
    fn shape_reads_config_and_weight_size() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("config.json"),
            r#"{"hidden_size":64,"num_hidden_layers":2,"vocab_size":100,"torch_dtype":"bfloat16"}"#,
        )
        .unwrap();
        std::fs::write(dir.path().join("model.safetensors"), vec![0u8; 2000]).unwrap();
        let s = model_shape(dir.path()).unwrap();
        assert_eq!((s.params, s.intermediate), (1000, 256));
        assert!(model_shape(&dir.path().join("missing")).is_err());
    }

    #[test]
    fn qlora_needs_a_working_cuda_probe() {
        let broken = Probe {
            devices: vec!["cpu".into(), "cuda".into()],
            qlora: false,
            qlora_reason: "bitsandbytes_unavailable".into(),
        };
        let c = compatibility(Some(&broken));
        let q = c.methods.iter().find(|m| m.method == "qlora").unwrap();
        assert_eq!(q.support, Support::Unsupported);
        let cpu_only = Probe {
            devices: vec!["cpu".into()],
            qlora: false,
            qlora_reason: "cuda_required".into(),
        };
        let c = compatibility(Some(&cpu_only));
        assert!(c.probed);
        let q = c.methods.iter().find(|m| m.method == "qlora").unwrap();
        assert_eq!(q.support, Support::Unsupported);
        assert_eq!(c.methods.len(), 5);
    }
}
