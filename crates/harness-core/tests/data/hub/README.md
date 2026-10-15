# Hub regression snapshots

Reduced from the G8 audit's cached Hugging Face API responses, trees, configs and GGUF header metadata on 2026-10-15. Filenames, sizes and selected config fields are preserved. VieNeu retains both GGUF variants, listing/detail family hints and the captured GGUF header to test the SEA-G2P refusal; Lightning retains root artifacts and omits split blocks. No model weights are included.

The ASR additions follow Transformers 5.19.0: the CTC and speech seq2seq mappings plus the four transducers handled in `pipelines/automatic_speech_recognition.py`.
