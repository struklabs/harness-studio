//! The sidecar's `--help` text.
//!
//! Harness reads it to learn which flags this build has (the same `[default] description`
//! table layout whisper-server prints), so a launch only sends flags the binary knows. Keep
//! one flag per line; `harness-core` checks its parameter catalog against this text.

/// Exit code of a launch with [`CUDA_FALLBACK_EXIT`] whose CUDA provider cannot be used:
/// Harness starts the model again on DirectML instead of leaving it on the CPU.
pub const EXIT_CUDA_UNUSABLE: i32 = 86;

/// Harness passes it with the CUDA build on Windows (where the DirectML build can take over).
pub const CUDA_FALLBACK_EXIT: &str = "--cuda-fallback-exit";

/// `harness-onnx --help`.
pub const HELP: &str = "\
harness-onnx: serves a Hugging Face ONNX model over HTTP: an encoder (embeddings, reranking), CTC speech recognizer or an onnxruntime-genai chat model

usage: harness-onnx --model <model.onnx> [options]

options:
  -h,  --help                         [default] show this help and exit
       --version                      [default] print the version and exit
  -m,  --model PATH                   [required] ONNX model file; tokenizer.json and sentence-transformers files, or CTC audio configs and vocab.json, are read from its repo folder (a folder with genai_config.json is a chat model)
       --host HOST                    [127.0.0.1] address to listen on
       --port N                       [8080] port to listen on
  -a,  --alias NAME                   [file name] model name for /v1/models and the responses
       --ort-lib PATH                 [onnxruntime] ONNX Runtime library to load (the managed runtime of Harness)
       --genai-lib PATH               [next to --ort-lib] onnxruntime-genai library to load for chat models
       --cuda-fallback-exit           [off] exit with code 86 when CUDA cannot be used, instead of running on the CPU
       --ctx-size N                   [4096] chat models: tokens of context (prompt plus answer), at most the model's own
       --provider NAME                [auto] execution provider: auto, cpu, directml or cuda (chat models: auto follows genai_config.json)
       --threads N                    [auto] CPU threads
       --task NAME                    [auto] embedding, rerank or asr_ctc (default: read from the repo files)
       --pooling NAME                 [auto] mean, cls, max or last (default: 1_Pooling/config.json)
       --normalize on|off             [auto] L2-normalize the embeddings (default: a Normalize module in modules.json)
       --max-length N                 [auto] maximum tokens per text (default: sentence_bert_config.json, tokenizer_config.json, config.json)
       --batch-size N                 [16] texts per model run

endpoints: GET /health, GET /v1/models, POST /v1/embeddings, POST /v1/rerank, POST /v1/chat/completions (chat models), POST /v1/audio/transcriptions (CTC models)
";

/// `harness-onnx --version`.
pub fn version() -> String {
    format!("harness-onnx {}", env!("CARGO_PKG_VERSION"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_flag_line_has_a_default_bracket() {
        for line in HELP.lines().filter(|l| l.trim_start().starts_with('-')) {
            assert!(line.contains('[') && line.contains(']'), "{line}");
        }
    }

    #[test]
    fn version_names_the_binary() {
        assert!(version().starts_with("harness-onnx 0."));
    }
}
