# Training: Fine-Tune a Model on Your Own Data

**What you do here:** teach a local model your own answers with LoRA fine-tuning, watch the loss, and put the result into your model library.

**Training** is an Advanced level page. It runs a supervised fine-tune (SFT) of a model from **My Models** on a JSONL file you choose, entirely on your computer. The result is a normal library model you can start and chat with.

## Before you start

- Switch to the **Advanced** interface level, then open **Training**.
- The first time, press **Install Training Environment**. It adds PyTorch, transformers, peft, trl and datasets to the managed Python environment (about 1.5 GB on CPU, about 3 GB with CUDA). Nothing else on your computer is changed.
- The base model must be a Hugging Face folder (safetensors with a `config.json`) inside one of your model folders. GGUF files cannot be trained.

## Dataset format

One JSON object per line, in either format (not mixed in one file):

```jsonl
{"messages": [{"role": "user", "content": "What is the capital of France?"}, {"role": "assistant", "content": "Paris."}]}
{"instruction": "Name the capital of Japan.", "input": "", "output": "Tokyo."}
```

- Chat rows need at least two messages, roles `system`, `user` or `assistant`, and must end with an assistant message.
- Instruction rows need `instruction` and `output`; `input` is optional.
- Rows may carry a `license` field. If none do, a `<file>.meta.json` next to the file with a `license` string is used. The wizard always shows the licence it found, and warns when it is unknown. The licence is also written to the exported model's `harness-training.json`.
- A validation share (10 % by default, at least one row) is held out. The check shows the format, row counts, the train/validation split, file size, estimated tokens and every problem with its line number. Files over 256 MB are refused.

## The wizard

1. **Base model:** pick from the library.
2. **Dataset:** choose the file; the check runs at once.
3. **Method and parameters:** LoRA or QLoRA, device (CPU, CUDA, or MPS on Apple Silicon), steps, learning rate, rank, sequence length, validation share.
4. **Review:** the memory and time estimate for this machine, and whether it fits. The numbers are planning estimates, not promises.

**Create and Start** queues the job; **Save as Draft** keeps it for later.

## Job lifecycle

`draft` -> `queued` -> `running` -> `done`, `failed` or `cancelled`.

- Only one training worker runs at a time. A GPU job also waits in the shared GPU queue (the same one chat and image models use), so training never takes VRAM from a model that is loaded.
- **Cancel** works at every stage. A running job saves a checkpoint first (killed after 90 seconds if it does not respond).
- **Resume** continues a failed or cancelled job from its last complete checkpoint, including optimizer state. A job interrupted by closing the app shows as failed and can be resumed.
- Loss and eval loss are stored for every step; the chart reads them. Everything a job owns (dataset snapshot, metrics, log, checkpoints, adapter) is in `<data folder>/training/<id>/`. **Delete** removes it; an exported model stays.

## Export to Library

**Export to Library** merges the LoRA into the base model and writes safetensors to `<models folder>/training/<name>/`. The library lists it like any downloaded model. Tick **Also convert to GGUF** to get a 16-bit GGUF in `<name>-GGUF/` as well; this needs the conversion environment from **Settings** (the llama.cpp converter). Without it, the safetensors export still works and a note explains why no GGUF was made. Use the Model Hub's quantize flow or `llama-quantize` for smaller GGUF types.

## What this machine can run

The **Method Compatibility** card tells you, for this computer, what works and why.

| Method | What it is | Here |
| --- | --- | --- |
| LoRA | Trains small adapter matrices; the base model stays frozen | Works on CPU, CUDA and MPS. On CPU it is slow and only small models fit. |
| QLoRA | LoRA on a 4-bit base model (bitsandbytes) | NVIDIA with CUDA only. Not on macOS, AMD or CPU. The card shows the cause when bitsandbytes does not load. |
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

Job changes are also published on the events stream as `training` and `training_removed`.
