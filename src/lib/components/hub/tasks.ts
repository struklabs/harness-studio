// Hugging Face tasks (`pipeline_tag`) offered in the Model Hub, grouped like huggingface.co/models.
// How far Harness Studio runs each one is not kept here: the backend derives it from the installed
// llama-server's capability registry (`get_task_support`, `harness_core::hub_tasks`), see
// `taskSupport.svelte.ts`. `harness-core` checks that both lists name the same tasks.
import type { TKey } from '$lib/i18n.svelte';

/** yes: runs here; partial: only some repos (e.g. with an audio/vision projector); no: not supported. */
export type TaskSupport = 'yes' | 'partial' | 'no';
export type TaskGroup = 'multimodal' | 'nlp' | 'audio' | 'cv';

export interface HubTask {
  /** HF `pipeline_tag`. */
  id: string;
  /** English name as Hugging Face shows it (tooltip). */
  hf: string;
  key: TKey;
  group: TaskGroup;
}

export const TASK_GROUPS: { id: TaskGroup; key: TKey }[] = [
  { id: 'multimodal', key: 'hub.task_group_multimodal' },
  { id: 'nlp', key: 'hub.task_group_nlp' },
  { id: 'audio', key: 'hub.task_group_audio' },
  { id: 'cv', key: 'hub.task_group_cv' },
];

export const TASKS: HubTask[] = [
  // Multimodal: vision / audio input through an mmproj projector.
  { id: 'image-text-to-text', hf: 'Image-Text-to-Text', key: 'hub.task_image_text_to_text', group: 'multimodal' },
  { id: 'audio-text-to-text', hf: 'Audio-Text-to-Text', key: 'hub.task_audio_text_to_text', group: 'multimodal' },
  { id: 'visual-question-answering', hf: 'Visual Question Answering', key: 'hub.task_visual_question_answering', group: 'multimodal' },
  { id: 'any-to-any', hf: 'Any-to-Any', key: 'hub.task_any_to_any', group: 'multimodal' },
  { id: 'document-question-answering', hf: 'Document Question Answering', key: 'hub.task_document_question_answering', group: 'multimodal' },
  { id: 'video-text-to-text', hf: 'Video-Text-to-Text', key: 'hub.task_video_text_to_text', group: 'multimodal' },
  // NLP: chat, embeddings (--embeddings), reranking (--rerank).
  { id: 'text-generation', hf: 'Text Generation', key: 'hub.task_text_generation', group: 'nlp' },
  { id: 'feature-extraction', hf: 'Feature Extraction', key: 'hub.task_feature_extraction', group: 'nlp' },
  { id: 'sentence-similarity', hf: 'Sentence Similarity', key: 'hub.task_sentence_similarity', group: 'nlp' },
  { id: 'text-ranking', hf: 'Text Ranking', key: 'hub.task_text_ranking', group: 'nlp' },
  { id: 'translation', hf: 'Translation', key: 'hub.task_translation', group: 'nlp' },
  { id: 'summarization', hf: 'Summarization', key: 'hub.task_summarization', group: 'nlp' },
  { id: 'question-answering', hf: 'Question Answering', key: 'hub.task_question_answering', group: 'nlp' },
  { id: 'text-classification', hf: 'Text Classification', key: 'hub.task_text_classification', group: 'nlp' },
  { id: 'token-classification', hf: 'Token Classification', key: 'hub.task_token_classification', group: 'nlp' },
  { id: 'zero-shot-classification', hf: 'Zero-Shot Classification', key: 'hub.task_zero_shot_classification', group: 'nlp' },
  { id: 'fill-mask', hf: 'Fill-Mask', key: 'hub.task_fill_mask', group: 'nlp' },
  { id: 'table-question-answering', hf: 'Table Question Answering', key: 'hub.task_table_question_answering', group: 'nlp' },
  // Audio: chat audio needs a projector; transcription also uses whisper.cpp or sherpa-onnx.
  { id: 'automatic-speech-recognition', hf: 'Automatic Speech Recognition', key: 'hub.task_automatic_speech_recognition', group: 'audio' },
  { id: 'text-to-speech', hf: 'Text-to-Speech', key: 'hub.task_text_to_speech', group: 'audio' },
  { id: 'text-to-audio', hf: 'Text-to-Audio', key: 'hub.task_text_to_audio', group: 'audio' },
  { id: 'audio-to-audio', hf: 'Audio-to-Audio', key: 'hub.task_audio_to_audio', group: 'audio' },
  { id: 'audio-classification', hf: 'Audio Classification', key: 'hub.task_audio_classification', group: 'audio' },
  // Computer vision: diffusion / detection GGUFs are for other runtimes (ComfyUI, stable-diffusion.cpp, ...).
  { id: 'image-to-text', hf: 'Image-to-Text', key: 'hub.task_image_to_text', group: 'cv' },
  { id: 'text-to-image', hf: 'Text-to-Image', key: 'hub.task_text_to_image', group: 'cv' },
  { id: 'image-to-image', hf: 'Image-to-Image', key: 'hub.task_image_to_image', group: 'cv' },
  { id: 'text-to-video', hf: 'Text-to-Video', key: 'hub.task_text_to_video', group: 'cv' },
  { id: 'image-to-video', hf: 'Image-to-Video', key: 'hub.task_image_to_video', group: 'cv' },
  { id: 'image-classification', hf: 'Image Classification', key: 'hub.task_image_classification', group: 'cv' },
  { id: 'object-detection', hf: 'Object Detection', key: 'hub.task_object_detection', group: 'cv' },
  { id: 'image-segmentation', hf: 'Image Segmentation', key: 'hub.task_image_segmentation', group: 'cv' },
  { id: 'depth-estimation', hf: 'Depth Estimation', key: 'hub.task_depth_estimation', group: 'cv' },
];

/** Always-visible task chips (the ones that run today). */
export const QUICK_TASKS = ['text-generation', 'image-text-to-text', 'feature-extraction', 'text-ranking'];

const BY_ID = new Map(TASKS.map((x) => [x.id, x]));

export function taskInfo(id: string | null | undefined): HubTask | null {
  return id ? (BY_ID.get(id) ?? null) : null;
}
