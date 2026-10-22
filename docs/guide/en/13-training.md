# Training: Fine-Tune a Model on Your Own Data

**What you do here:** teach a local model your own answers with LoRA fine-tuning, watch the loss, and put the result into your model library.

**Training** is an Advanced level page. It runs a supervised fine-tune (SFT) of a model from **My Models** on a JSONL file you choose, entirely on your computer. The result is a normal library model you can start and chat with.

## Before you start

- Switch to the **Advanced** interface level, then open **Training**.
- The training environment installs automatically when needed to start or resume a run. You can also prepare it with **Install Training Environment**. It adds PyTorch, transformers, peft, trl and datasets to the managed Python environment (about 1.5 GB on CPU, about 3 GB with CUDA).
- The base model must be a Hugging Face folder (safetensors with a `config.json`) inside one of your model folders. GGUF files cannot be trained.
- On an Apple Silicon Mac, jobs train with **MLX** by default (mlx-lm on the GPU). MLX needs none of the PyTorch environment: the first MLX job installs the MLX engine itself if it is missing. It trains MLX model folders (also 4-bit ones such as `mlx-community/...-4bit`) and Hugging Face folders of the architectures mlx-lm supports. **Apple Silicon (MPS)** stays available as the PyTorch device.

## Dataset format

Choose a **file** (JSONL, JSON array or CSV) or a **folder** of such files. The format is detected for every row and mapped to chat messages; the wizard shows which field became the question and which the answer. One file uses one format:

```jsonl
{"messages": [{"role": "user", "content": "What is the capital of France?"}, {"role": "assistant", "content": "Paris."}]}
{"conversations": [{"from": "human", "value": "Name the capital of Japan."}, {"from": "gpt", "value": "Tokyo."}]}
{"instruction": "Name the capital of Spain.", "input": "", "output": "Madrid."}
{"prompt": "Capital of Italy?", "completion": "Rome."}
```

- **Chat** (`messages`): at least two messages, roles `system`, `user` or `assistant`, ending with an assistant message.
- **Conversations** (`conversations` with `from` and `value`): `human`/`user`, `gpt`/`assistant` and `system` turns, ending with the assistant.
- **Instruction** (`instruction`, optional `input`, and `output`/`response`/`answer`): input is appended to the instruction.
- **Prompt and completion**: `prompt`, `question`, `query` or `input` with `completion`, `response`, `answer`, `output` or `target`. CSV headers use the same names (case-insensitive).
- Rows may carry a `license` field. If none do, a `<file>.meta.json` next to the file with a `license` string is used. The wizard always shows the licence it found, and warns when it is unknown. The licence is also written to the exported model's `harness-training.json`.
- The check shows the format and mapping, row counts, the train/evaluation split, file size, estimated tokens, a **length histogram** (bars longer than the chosen sequence length are marked, because those examples get cut), the **first sample rows** as the model will see them, and every problem with its line number. Data over 256 MB is refused. Parquet files and Hugging Face dataset ids are not supported yet; export the data as JSONL or CSV.
- Drag the **Evaluation Split** slider to hold out 1 to 50 % of the rows (at least one) to measure progress on data the model has not trained on.

The job trains on a normalized snapshot (chat messages) kept in its folder, so the original files can change or move afterwards.

## The wizard

A switch at the top selects **Simple** or **Advanced** mode (remembered). Simple mode hides the details and picks safe values for your hardware; Advanced mode shows every setting with a help tooltip, the recommended value next to it and a reset button.

1. **Base model:** pick from the library (search by name; size and architecture shown), or type a folder path. You can also paste a recipe copied from an earlier run to start from its settings.
2. **Dataset:** as above.
3. **Method** (Advanced only): **Automatic** (recommended), **LoRA** or **QLoRA**. Methods this machine or the training engine cannot run (full fine-tuning, DPO) are listed with the reason instead of being offered.
4. **Settings:** three hardware-aware presets, **Light**, **Balanced** (recommended) and **Quality**. They are computed from the model, your dataset and the memory free on the training device (VRAM on a GPU, RAM on a CPU or with MPS, the unified-memory budget with MLX): method, sequence length (never longer than your data needs), LoRA rank, batch size and gradient accumulation, steps, and the memory and time estimate with a fits / tight / will-not-fit verdict. Advanced mode adds steps, learning rate, sequence length, rank, alpha, dropout, batch size, gradient accumulation, warm-up, learning-rate schedule, weight decay and checkpoint/evaluation intervals.
5. **Review:** name the model (a name is suggested from the model and dataset, checked live against other runs and the model library; it is the name the finished model has in the Library), see the memory and time estimate, and copy the run's **recipe** (JSON) to reproduce or share it. Optionally add **checkpoint prompts**, one per line (up to eight, each up to 2000 characters), then refresh the summary. These prompts are kept in the recipe and generate sample answers at saved checkpoints.

**Create and Start** queues the job (and installs the training engine first when it is missing, without asking); **Save as Draft** keeps it for later. A run can be **renamed** at any time from the job list, and **Copy Recipe** and the run's settings are on every job.

## Job lifecycle

`draft` -> `queued` -> `running` -> `done`, `failed` or `cancelled`.

- Only one training worker runs at a time. A GPU job also waits in the shared GPU queue (the same one chat and image models use), so training never takes VRAM from a model that is loaded.
- **Cancel** works at every stage. A running job saves a checkpoint first (killed after 90 seconds if it does not respond).
- **Pause** saves a checkpoint at the next training step and releases the worker slot. Wait for **Paused**, then use **Resume** to continue. If saving fails or times out, the run may only have an older complete checkpoint.
- **Resume** continues a failed or cancelled job from its last complete checkpoint, including optimizer state. A job interrupted by closing the app shows as failed and can be resumed.
- Loss and eval loss are stored for every step; the chart reads them. Everything a job owns (dataset snapshot, metrics, log, checkpoints, adapter) is in `<data folder>/training/<id>/`. **Delete** removes it; an exported model stays.
- Select a run to see its live loss/evaluation curves, device memory and checkpoint generations. CUDA reports used/total device VRAM; MPS reports memory allocated by the worker; MLX reports the unified memory it holds against the GPU's recommended working set. CPU runs show that device memory measurement is unavailable. Expand a checkpoint to read each prompt and its answer.

## Export to Library

**Export to Library** merges the LoRA into the base model and writes safetensors to `<models folder>/training/<name>/`. The Library lists it like any downloaded model. Existing exports are preserved; repeated names receive a numeric suffix.

**Also convert to GGUF** is selected by default. Choose one or several of **Q4_K_M**, **Q5_K_M**, **Q8_0**, and **F16**, then start the export. The dialog shows an approximate size for each choice; actual size depends on tensor shapes and tokenizer overhead. Q4_K_M is smallest, Q5_K_M retains more precision, and Q8_0 is larger. Export merges once, converts once to F16, then uses the managed engine's `llama-quantize` for each selected quantized type. Allow space for the merged safetensors, F16 intermediate and all selected GGUFs.

The desktop export automatically installs a missing managed engine, conversion scripts and Python dependencies without a confirmation prompt. Conversion uses Harness's removable environment, matched to that engine's release. The finished GGUFs appear in `<name>-GGUF/` under the run's name, each as a Library model. Use **Load and Chat** beside a file to load that quant and open Playground, or run `harness load <path-to-gguf>`. If GGUF conversion fails, export shows a failure and keeps the merged safetensors folder so it can still be used; incomplete GGUF batches are not published.

An MLX job's merged export is an MLX model folder (a 4-bit base stays 4-bit), which the MLX engine loads. With **Also convert to GGUF**, it is written in full precision first so llama.cpp's converter can read it.

Choose **Export LoRA adapter only** to copy `adapter_model.safetensors` (MLX: `adapters.safetensors`) and `adapter_config.json` without merging or copying base weights. An MLX adapter cannot become a GGUF: llama.cpp's LoRA converter reads PEFT adapters, so export shows a warning and keeps the MLX adapter. With GGUF enabled, Harness also attempts an F16 GGUF LoRA adapter in the export's `gguf/` subfolder. Support depends on the architecture and llama.cpp release; an unsupported conversion leaves the safetensors adapter and a warning. An adapter is not a standalone model: use the exact base model and attach the GGUF adapter through the model's LoRA settings (or `harness lora`). The export includes provenance in `harness-training.json`, including the base model path and dataset licence. Keep or distribute the base model separately according to its licence.

## Before and after

Use **Before and After** on a finished run, or a stopped run with a complete checkpoint, to open its comparison in **Playground**. Enter one prompt per line and press **Compare**. The run's checkpoint prompts are filled in when available. Each prompt gets a base answer and a fine-tuned answer side by side, using the same chat template, context limit, greedy decoding and a 128-token output limit. Comparison uses the run's saved adapter, including before export. Both passes run one after the other, on the CPU for PyTorch runs and on the Apple Silicon GPU for MLX runs (one model in memory at a time), which can take time for larger models; the training slot is held during comparison. Answers remain in the panel until it closes or another comparison starts.

## Training CLI

Start `harness serve` (or enable the desktop Control API) before starting, resuming, cancelling or exporting a job. These commands use that instance so workers keep running after the CLI returns. `list` and `status` can also read persisted jobs when no instance is running.

```sh
harness training list
harness training start training-spec.json
harness training status 1
harness training cancel 1
harness training resume 1
harness training export 1 --gguf --quant Q4_K_M
harness training export 1 --adapter-only --gguf --name my-adapter
```

A JSON spec uses the same fields as `POST /training`. `model` must be a local safetensors folder in the Library; `dataset` is a JSONL path on the instance's computer. For example:

```json
{
  "name": "My SmolLM2",
  "model": "/models/HuggingFaceTB/SmolLM2-135M-Instruct",
  "dataset": "/datasets/answers.jsonl",
  "method": "lora",
  "device": "cpu",
  "steps": 5,
  "rank": 8,
  "sequence_length": 128
}
```

`start`, `resume` and `export` return after accepting the action; poll `status` for the result. Export status is separate from training status. Use `--json` for machine-readable output. Install the training environment from the Training page or `POST /training/env` before starting a job. For a remote instance, pass `--host` and an admin token.

## What this machine can run

The **Method Compatibility** card tells you, for this computer, what works and why.

| Method | What it is | Here |
| --- | --- | --- |
| LoRA | Trains small adapter matrices; the base model stays frozen | Works on CPU, CUDA, MPS and MLX. On CPU it is slow and only small models fit. With MLX a 4-bit MLX model trains at its 4 bits. |
| QLoRA | LoRA on a 4-bit base model | NVIDIA with CUDA (bitsandbytes), or MLX on Apple Silicon (a full-precision model is quantized to 4 bits for training). Not with MPS, AMD or CPU. The card shows the cause when bitsandbytes does not load. |
| SFT | Supervised fine-tuning on prompt and answer pairs | The objective every job uses. |
| DPO | Preference tuning from chosen/rejected pairs | Not offered yet; needs a preference dataset and a reference model. |
| Full fine-tuning | Updates every weight | Not offered. It needs about 16 bytes per parameter (weights, gradients, Adam state), far more than LoRA. |

## Control API

All endpoints are under `/api/harness/v1`. Reads need `read`; everything that changes state needs `admin`.

| Endpoint | Purpose |
| --- | --- |
| `GET /training`, `POST /training` | List jobs; create a draft |
| `POST /training/dataset` | Check a dataset file |
| `POST /training/preview` | Configuration, dataset report and estimate for a request |
| `GET /training/compatibility` | Method compatibility of this machine |
| `GET`, `POST /training/env` | Training environment status; install it |
| `GET`, `DELETE /training/{id}` | One job; delete it |
| `GET /training/{id}/metrics`, `GET /training/{id}/log` | Loss history; worker output tail |
| `POST /training/{id}/start`, `/cancel`, `/resume`, `/export` | Lifecycle and export |
| `POST /training/{id}/pause` | Save a checkpoint and release the worker slot; use `/resume` afterwards |
| `POST /training/{id}/compare` | Before/after answers; body: `{"prompts":["Your prompt"]}` (admin required) |

Job changes are also published on the events stream as `training` and `training_removed`.

The export body accepts `name` (optional), `gguf` (default `false`), `quant` (`Q4_K_M` by default; also `Q5_K_M`, `Q8_0`, `F16`), `quants` (optional array for multiple types), and `adapter_only` (default `false`). Empty or omitted `quants` preserves the single `quant` behavior. Adapter GGUFs always use F16. Export results include `ggufs` for the batch and keep `gguf` as its first file for existing clients. Job requests accept optional `sample_prompts`; metrics include optional device memory readings and checkpoint `samples`.
