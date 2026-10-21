//! Hardware-aware presets: from the model, the dataset and the memory this machine has free on
//! the training device, pick the method, sequence length, LoRA rank, batch size and gradient
//! accumulation that fit, and offer three levels (light, balanced, quality). Nothing here needs
//! a GPU vendor: the same search runs for CUDA, Apple Silicon and CPU, against that device's
//! free memory.

use super::estimate::{self, Estimate, ModelShape};
use super::{dataset, Device, LrScheduler, Method};
use serde::{Deserialize, Serialize};
use std::path::Path;

/// Effective batch the search aims for when the dataset is large enough.
const TARGET_BATCH: u64 = 16;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[cfg_attr(feature = "openapi", schema(as = TrainingPresetTier))]
pub enum PresetTier {
    /// Smallest memory and shortest run.
    Light,
    /// The recommended default: leaves headroom for the desktop and other models.
    Balanced,
    /// Longest sequences and highest rank that still fit.
    Quality,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[cfg_attr(feature = "openapi", schema(as = TrainingFitLevel))]
pub enum FitLevel {
    Fits,
    Tight,
    No,
}

/// Body of a preset request. The dataset is optional: without it a typical size is assumed.
#[derive(Debug, Clone, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[cfg_attr(feature = "openapi", schema(as = TrainingPresetRequest))]
pub struct PresetRequest {
    pub model: String,
    pub dataset: Option<String>,
    pub device: Option<Device>,
    /// Only offer this method (default: the best of LoRA and QLoRA for this machine).
    pub method: Option<Method>,
    pub validation_fraction: Option<f64>,
}

/// One set of values, ready to be sent as a job request, with what it costs on this machine.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[cfg_attr(feature = "openapi", schema(as = TrainingPreset))]
pub struct Preset {
    pub tier: PresetTier,
    pub method: Method,
    pub device: Device,
    pub batch_size: u32,
    pub grad_accum: u32,
    pub sequence_length: u32,
    pub rank: u32,
    pub lora_alpha: u32,
    pub learning_rate: f64,
    pub steps: u32,
    pub warmup_ratio: f64,
    pub lr_scheduler: LrScheduler,
    pub lora_dropout: f64,
    pub weight_decay: f64,
    pub estimate: Estimate,
    pub fit: FitLevel,
    /// The preset to start with.
    pub recommended: bool,
    /// i18n keys (`training.preset_note_*`) that explain a trade-off.
    pub notes: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[cfg_attr(feature = "openapi", schema(as = TrainingPresetSet))]
pub struct PresetSet {
    pub device: Device,
    /// Memory free on the device now.
    pub available_bytes: u64,
    pub presets: Vec<Preset>,
}

/// What the search knows about the data.
#[derive(Debug, Clone, Copy)]
pub struct DataShape {
    pub avg_row_tokens: u64,
    pub max_row_tokens: u64,
    pub train_rows: u64,
}

impl Default for DataShape {
    fn default() -> Self {
        Self {
            avg_row_tokens: 256,
            max_row_tokens: 512,
            train_rows: 500,
        }
    }
}

fn fit_level(estimate: &Estimate) -> FitLevel {
    if estimate.available_bytes == 0 {
        return FitLevel::Fits;
    }
    let ratio = estimate.memory_bytes as f64 / estimate.available_bytes as f64;
    if ratio <= 0.75 {
        FitLevel::Fits
    } else if ratio <= 1.0 {
        FitLevel::Tight
    } else {
        FitLevel::No
    }
}

fn next_pow2(n: u64) -> u64 {
    n.max(1).next_power_of_two()
}

fn prev_pow2(n: u64) -> u64 {
    if n == 0 {
        1
    } else {
        1 << (63 - n.leading_zeros())
    }
}

/// Longest sequence the data asks for, on a power-of-two grid.
fn wanted_sequence(data: &DataShape) -> u32 {
    next_pow2(data.max_row_tokens).clamp(128, 2048) as u32
}

struct Candidate {
    method: Method,
    sequence: u32,
    rank: u32,
    batch: u32,
}

fn candidates(
    device: Device,
    qlora: bool,
    force: Option<Method>,
    data: &DataShape,
) -> Vec<Candidate> {
    let want = wanted_sequence(data);
    // (sequence, rank, batch) from the best quality down.
    const LADDER: [(u32, u32, u32); 8] = [
        (2048, 32, 4),
        (1024, 32, 2),
        (1024, 16, 2),
        (512, 16, 2),
        (512, 16, 1),
        (256, 8, 1),
        (128, 8, 1),
        (128, 4, 1),
    ];
    let mut methods = vec![Method::Lora];
    if qlora && device.trains_quantized() {
        methods.push(Method::Qlora);
    }
    if let Some(m) = force {
        methods = vec![m];
    }
    let mut out: Vec<Candidate> = Vec::new();
    for (seq, rank, batch) in LADDER {
        for method in &methods {
            let sequence = seq.min(want);
            if out.iter().any(|c| {
                c.method == *method && c.sequence == sequence && c.rank == rank && c.batch == batch
            }) {
                continue;
            }
            out.push(Candidate {
                method: *method,
                sequence,
                rank,
                batch,
            });
        }
    }
    out
}

fn build(
    c: &Candidate,
    tier: PresetTier,
    shape: &ModelShape,
    device: Device,
    data: &DataShape,
    available: u64,
) -> Preset {
    let (epochs, scheduler) = match tier {
        PresetTier::Light => (1, LrScheduler::Linear),
        PresetTier::Balanced => (2, LrScheduler::Cosine),
        PresetTier::Quality => (3, LrScheduler::Cosine),
    };
    // Effective batch: 16 where the data allows, fewer for small datasets so that a run still
    // has enough updates.
    let eff = prev_pow2(data.train_rows / 8).clamp(1, TARGET_BATCH) as u32;
    let accum = (eff / c.batch).max(1);
    let per_update = u64::from(c.batch) * u64::from(accum);
    let steps = (epochs * data.train_rows)
        .div_ceil(per_update.max(1))
        .clamp(20, 5000) as u32;
    let estimate = estimate::estimate_with(
        shape,
        c.method,
        device,
        c.rank,
        c.sequence,
        steps,
        data.avg_row_tokens,
        c.batch,
        accum,
        available,
    );
    let mut notes = Vec::new();
    if c.method == Method::Qlora {
        notes.push("training.preset_note_qlora".to_string());
    }
    if c.sequence < wanted_sequence(data) {
        notes.push("training.preset_note_short_sequence".to_string());
    }
    let fit = fit_level(&estimate);
    if fit == FitLevel::No {
        notes.push("training.preset_note_too_big".to_string());
    }
    Preset {
        tier,
        method: c.method,
        device,
        batch_size: c.batch,
        grad_accum: accum,
        sequence_length: c.sequence,
        rank: c.rank,
        lora_alpha: c.rank * 2,
        learning_rate: 2e-4,
        steps,
        warmup_ratio: 0.05,
        lr_scheduler: scheduler,
        lora_dropout: 0.05,
        weight_decay: 0.01,
        estimate,
        fit,
        recommended: false,
        notes,
    }
}

/// The presets for a model on `device` with `available` bytes free. `qlora` says whether
/// 4-bit training works here. Always returns the balanced preset first (recommended), then
/// the others that differ from it.
pub fn choose(
    shape: &ModelShape,
    device: Device,
    available: u64,
    data: &DataShape,
    qlora: bool,
    force: Option<Method>,
) -> Vec<Preset> {
    let list = candidates(device, qlora, force, data);
    let costed: Vec<(&Candidate, Estimate)> = list
        .iter()
        .map(|c| {
            let est = estimate::estimate_with(
                shape,
                c.method,
                device,
                c.rank,
                c.sequence,
                1,
                data.avg_row_tokens,
                c.batch,
                1,
                available,
            );
            (c, est)
        })
        .collect();
    // Unknown free memory (0) means no limit.
    let within = |est: &Estimate, share: f64| {
        available == 0 || (est.memory_bytes as f64) <= available as f64 * share
    };
    let smallest = costed
        .iter()
        .min_by_key(|(_, e)| e.memory_bytes)
        .map(|(c, _)| *c)
        .expect("the ladder is not empty");
    let quality = costed
        .iter()
        .find(|(_, e)| within(e, 0.9))
        .map(|(c, _)| *c)
        .unwrap_or(smallest);
    let balanced = costed
        .iter()
        .find(|(c, e)| within(e, 0.6) && c.sequence <= 1024 && c.rank <= 32)
        .map(|(c, _)| *c)
        .unwrap_or(quality);

    let mut out: Vec<Preset> = Vec::new();
    for (c, tier) in [
        (balanced, PresetTier::Balanced),
        (smallest, PresetTier::Light),
        (quality, PresetTier::Quality),
    ] {
        let preset = build(c, tier, shape, device, data, available);
        let same = |p: &Preset| {
            p.method == preset.method
                && p.sequence_length == preset.sequence_length
                && p.rank == preset.rank
                && p.batch_size == preset.batch_size
        };
        if tier != PresetTier::Balanced && out.iter().any(same) {
            continue;
        }
        out.push(preset);
    }
    out[0].recommended = true;
    out
}

/// Presets for a request: reads the model and the dataset, and asks this machine for its free
/// memory and whether QLoRA works.
pub async fn presets(req: &PresetRequest) -> Result<PresetSet, String> {
    let model = super::resolve_model(&req.model)?;
    let device = super::resolve_device(req.device)?;
    let shape = estimate::model_shape(&model)?;
    let data = match req
        .dataset
        .as_deref()
        .map(str::trim)
        .filter(|d| !d.is_empty())
    {
        Some(path) => {
            let path = path.to_string();
            let fraction = req.validation_fraction.unwrap_or(0.1);
            let report =
                tokio::task::spawn_blocking(move || dataset::inspect(Path::new(&path), fraction))
                    .await
                    .map_err(|e| e.to_string())?;
            match report.estimated_tokens.checked_div(report.rows) {
                Some(avg_row_tokens) => DataShape {
                    avg_row_tokens,
                    max_row_tokens: report.max_row_tokens,
                    train_rows: report.train_rows,
                },
                None => DataShape::default(),
            }
        }
        None => DataShape::default(),
    };
    // Presets never wait for the environment probe (a cold one takes minutes).
    let qlora = super::compatibility_now()
        .methods
        .iter()
        .any(|m| m.method == "qlora" && m.support != estimate::Support::Unsupported);
    if req.method == Some(Method::Qlora) && !(qlora && device.trains_quantized()) {
        return Err(crate::error_key::err("training.qlora_needs_cuda").into());
    }
    let available = estimate::available_bytes(device);
    Ok(PresetSet {
        device,
        available_bytes: available,
        presets: choose(&shape, device, available, &data, qlora, req.method),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const GB: u64 = 1024 * 1024 * 1024;

    fn small_model() -> ModelShape {
        // About 0.5B parameters (Qwen-like).
        ModelShape {
            params: 500_000_000,
            hidden: 896,
            layers: 24,
            intermediate: 4864,
            vocab: 151_000,
            quant_bits: None,
        }
    }

    fn seven_b() -> ModelShape {
        ModelShape {
            params: 7_000_000_000,
            hidden: 4096,
            layers: 32,
            intermediate: 11008,
            vocab: 32_000,
            quant_bits: None,
        }
    }

    fn data() -> DataShape {
        DataShape {
            avg_row_tokens: 200,
            max_row_tokens: 900,
            train_rows: 1000,
        }
    }

    #[test]
    fn roomy_gpu_gets_long_sequences_and_a_recommended_balanced_preset() {
        let set = choose(&small_model(), Device::Cuda, 24 * GB, &data(), true, None);
        assert_eq!(set[0].tier, PresetTier::Balanced);
        assert!(set[0].recommended);
        assert_eq!(set[0].method, Method::Lora);
        assert_eq!(set[0].fit, FitLevel::Fits);
        assert_eq!(set.iter().filter(|p| p.recommended).count(), 1);
        let q = set.iter().find(|p| p.tier == PresetTier::Quality);
        if let Some(q) = q {
            assert!(q.sequence_length >= set[0].sequence_length);
        }
    }

    #[test]
    fn sequence_never_exceeds_what_the_data_needs() {
        let short = DataShape {
            avg_row_tokens: 30,
            max_row_tokens: 60,
            train_rows: 400,
        };
        for p in choose(&small_model(), Device::Cuda, 48 * GB, &short, true, None) {
            assert_eq!(p.sequence_length, 128);
        }
    }

    #[test]
    fn tight_vram_switches_to_qlora_only_when_it_works() {
        let with = choose(&seven_b(), Device::Cuda, 8 * GB, &data(), true, None);
        assert_eq!(with[0].method, Method::Qlora);
        assert!(with[0]
            .notes
            .iter()
            .any(|n| n == "training.preset_note_qlora"));
        let without = choose(&seven_b(), Device::Cuda, 8 * GB, &data(), false, None);
        assert!(without.iter().all(|p| p.method == Method::Lora));
        assert_eq!(without[0].fit, FitLevel::No);
        assert!(without[0]
            .notes
            .iter()
            .any(|n| n == "training.preset_note_too_big"));
    }

    #[test]
    fn qlora_is_offered_only_where_it_trains() {
        for device in [Device::Cpu, Device::Mps] {
            let set = choose(&seven_b(), device, 16 * GB, &data(), true, None);
            assert!(set.iter().all(|p| p.method == Method::Lora));
        }
        // MLX quantizes the base itself: a 7B fits an 8 GB Mac's budget only that way.
        let set = choose(&seven_b(), Device::Mlx, 5 * GB, &data(), true, None);
        assert_eq!(set[0].method, Method::Qlora);
        assert!(set.iter().all(|p| p.device == Device::Mlx));
    }

    #[test]
    fn cpu_uses_its_own_memory_and_small_batches() {
        let set = choose(&small_model(), Device::Cpu, 6 * GB, &data(), false, None);
        assert!(set[0].estimate.memory_bytes <= 6 * GB);
        assert!(set.iter().all(|p| p.device == Device::Cpu));
    }

    #[test]
    fn effective_batch_and_steps_follow_the_dataset_size() {
        let tiny = DataShape {
            avg_row_tokens: 100,
            max_row_tokens: 200,
            train_rows: 40,
        };
        let p = &choose(&small_model(), Device::Cuda, 24 * GB, &tiny, false, None)[0];
        assert!(u64::from(p.batch_size * p.grad_accum) <= 8);
        assert!(p.steps >= 20);
        let big = DataShape {
            train_rows: 20_000,
            ..data()
        };
        let p = &choose(&small_model(), Device::Cuda, 24 * GB, &big, false, None)[0];
        assert_eq!(p.batch_size * p.grad_accum, 16);
        assert!(p.steps > 1000 && p.steps <= 5000);
        assert_eq!(p.lora_alpha, p.rank * 2);
    }

    #[test]
    fn unknown_free_memory_does_not_block() {
        let set = choose(&seven_b(), Device::Cuda, 0, &data(), false, None);
        assert_eq!(set[0].fit, FitLevel::Fits);
    }

    #[test]
    fn a_forced_method_limits_the_candidates() {
        let lora = choose(
            &small_model(),
            Device::Cuda,
            24 * GB,
            &data(),
            true,
            Some(Method::Lora),
        );
        assert!(lora.iter().all(|p| p.method == Method::Lora));
        let q = choose(
            &small_model(),
            Device::Cuda,
            24 * GB,
            &data(),
            true,
            Some(Method::Qlora),
        );
        assert!(q.iter().all(|p| p.method == Method::Qlora));
        assert!(q[0].estimate.memory_bytes < lora[0].estimate.memory_bytes);
    }
}
