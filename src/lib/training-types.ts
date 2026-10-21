// Mirrors harness_core::training (snake_case as serialized).

export type TrainingMethod = 'lora' | 'qlora';
export type TrainingDevice = 'cpu' | 'cuda' | 'mps';
export type TrainingStatus = 'draft' | 'queued' | 'running' | 'done' | 'failed' | 'cancelled';
export type DatasetFormat = 'chat' | 'instruction';
export type IssueLevel = 'error' | 'warning';
export type ExportStatus = 'running' | 'done' | 'failed';
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
  warning: string | null;
  error: string | null;
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
  error: string | null;
  dataset_report: DatasetReport;
  estimate: TrainingEstimate;
  export: TrainingExport | null;
  resumed: boolean;
  created_at: number;
  updated_at: number;
  started_at: number | null;
  finished_at: number | null;
}

export interface TrainingMetric {
  step: number;
  loss: number | null;
  eval_loss: number | null;
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
