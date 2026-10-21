// Mirrors harness_core::training (snake_case as serialized).

export type TrainingMethod = 'lora' | 'qlora';
export type TrainingDevice = 'cpu' | 'cuda' | 'mps' | 'mlx';
export type TrainingStatus = 'draft' | 'queued' | 'running' | 'done' | 'failed' | 'cancelled';
export type DatasetFormat = 'chat' | 'instruction' | 'sharegpt' | 'prompt_completion';
export type TrainingLrScheduler = 'linear' | 'cosine' | 'constant';
export type TrainingPresetTier = 'light' | 'balanced' | 'quality';
export type TrainingFitLevel = 'fits' | 'tight' | 'no';
export type IssueLevel = 'error' | 'warning';
export type ExportStatus = 'running' | 'done' | 'failed';
export type TrainingQuant = 'F16' | 'Q8_0' | 'Q4_K_M' | 'Q5_K_M';
export interface TrainingExportRequest {
  name: string | null;
  gguf: boolean;
  quant: TrainingQuant;
  adapter_only: boolean;
  quants?: TrainingQuant[];
}
export type SupportLevel = 'supported' | 'limited' | 'unsupported';

export interface TrainingConfig {
  name: string;
  model: string;
  dataset: string;
  method: TrainingMethod;
  device: TrainingDevice;
  steps: number;
  learning_rate: number;
  rank: number;
  sequence_length: number;
  validation_fraction: number;
  checkpoint_steps: number;
  eval_steps: number;
  batch_size: number;
  grad_accum: number;
  lora_alpha: number;
  lora_dropout: number;
  warmup_ratio: number;
  lr_scheduler: TrainingLrScheduler;
  weight_decay: number;
  sample_prompts?: string[];
}

export interface TrainingCreateRequest {
  name?: string;
  model: string;
  dataset: string;
  method?: TrainingMethod;
  device?: TrainingDevice;
  steps?: number;
  learning_rate?: number;
  rank?: number;
  sequence_length?: number;
  validation_fraction?: number;
  checkpoint_steps?: number;
  eval_steps?: number;
  batch_size?: number;
  grad_accum?: number;
  lora_alpha?: number;
  lora_dropout?: number;
  warmup_ratio?: number;
  lr_scheduler?: TrainingLrScheduler;
  weight_decay?: number;
  sample_prompts?: string[];
}

export interface DatasetIssue {
  level: IssueLevel;
  key: string;
  line: number | null;
  params: Record<string, string>;
}

export interface DatasetReport {
  path: string;
  format: DatasetFormat | null;
  rows: number;
  train_rows: number;
  val_rows: number;
  bytes: number;
  estimated_tokens: number;
  max_row_tokens: number;
  license: string | null;
  issues: DatasetIssue[];
  ok: boolean;
  /** Rows per length bucket: edges 32, 64 ... 8192 (estimated tokens), then longer rows. */
  histogram?: number[];
  samples?: DatasetSample[];
  mapping?: DatasetMapping[];
}

export interface DatasetSample {
  messages: { role: string; content: string }[];
  tokens: number;
}

export interface DatasetMapping {
  role: string;
  field: string;
}

export interface TrainingPreset {
  tier: TrainingPresetTier;
  method: TrainingMethod;
  device: TrainingDevice;
  batch_size: number;
  grad_accum: number;
  sequence_length: number;
  rank: number;
  lora_alpha: number;
  learning_rate: number;
  steps: number;
  warmup_ratio: number;
  lr_scheduler: TrainingLrScheduler;
  lora_dropout: number;
  weight_decay: number;
  estimate: TrainingEstimate;
  fit: TrainingFitLevel;
  recommended: boolean;
  notes: string[];
}

export interface TrainingPresetSet {
  device: TrainingDevice;
  available_bytes: number;
  presets: TrainingPreset[];
}

export interface TrainingNameCheck {
  name: string;
  available: boolean;
  taken_by: 'job' | 'library' | null;
  suggested: string;
}

export interface TrainingEstimate {
  params: number;
  memory_bytes: number;
  available_bytes: number;
  fits: boolean;
  seconds: number;
  seconds_per_step: number;
  tokens_per_step: number;
}

export interface TrainingExport {
  status: ExportStatus;
  dir: string | null;
  gguf: string | null;
  ggufs?: string[];
  warning: string | null;
  error: string | null;
  adapter_only: boolean;
}

export interface TrainingJob {
  id: number;
  config: TrainingConfig;
  status: TrainingStatus;
  step: number;
  total_steps: number;
  loss: number | null;
  eval_loss: number | null;
  checkpoint_step: number | null;
  training: boolean;
  /** When this run's worker finished loading (unix seconds). */
  training_since?: number | null;
  /** The step this run started training from. */
  training_from_step?: number;
  error: string | null;
  dataset_report: DatasetReport;
  estimate: TrainingEstimate;
  export: TrainingExport | null;
  resumed: boolean;
  created_at: number;
  updated_at: number;
  started_at: number | null;
  finished_at: number | null;
  paused?: boolean;
}

export interface TrainingMetric {
  step: number;
  loss: number | null;
  eval_loss: number | null;
  vram_bytes?: number | null;
  vram_total_bytes?: number | null;
  samples?: { prompt: string; text: string }[];
}

export interface TrainingComparisonRow {
  prompt: string;
  base: string;
  trained: string;
}

export interface MethodSupport {
  method: 'lora' | 'qlora' | 'sft' | 'dpo' | 'full';
  support: SupportLevel;
  reason: string;
  params: Record<string, string>;
}

export interface TrainingCompatibility {
  os: string;
  devices: TrainingDevice[];
  probed: boolean;
  methods: MethodSupport[];
}

export interface TrainingEnvProgress {
  step: string;
  detail: string;
  downloaded_bytes: number;
  total_bytes: number;
}

export interface TrainingEnvStatus {
  name: string;
  supported: boolean;
  refusal: string | null;
  installed: boolean;
  ready: boolean;
  size_bytes: number;
  estimated_size_bytes: number;
  progress: TrainingEnvProgress | null;
}

export type TrainingPreview = [TrainingConfig, DatasetReport, TrainingEstimate];
