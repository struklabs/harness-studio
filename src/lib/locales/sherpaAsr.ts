// Speech recognition with sherpa-onnx models: the model picker groups, the catalog of
// recommended downloads in the Playground Speech tab and the library kind label. The backend
// error keys (stt.*) are in errorsRuntime.ts.
const tr = {
  'sasr.group_definitions': 'Kütüphane modelleri',
  'sasr.group_sherpa': 'sherpa-onnx',
  'sasr.group_mlx': 'MLX (mlx-audio)',
  'sasr.group_python': 'Python Motoru',
  'sasr.text_only': 'Bu model düz metin döndürür; altyazı (SRT, WebVTT) çıkarmaz.',
  'sasr.kind': 'Konuşma tanıma (sherpa-onnx)',
  'sasr.catalog_title': 'Önerilen sherpa-onnx Tanıma Modelleri',
  'sasr.catalog_desc': 'Whisper’ın çalıştıramadığı ONNX modelleri: NVIDIA Parakeet, SenseVoice ve Türkçe dahil 1600 dili tanıyan Omnilingual. Uzun kayıtlar duraklardan bölünür ve her parçanın zamanı yazılır.',
  'sasr.no_runtime': 'sherpa-onnx model motoru kurulu değil. Bu modelleri çalıştıran küçük bir araçtır (yaklaşık 20 MB, resmi sürümden SHA-256 ile doğrulanarak indirilir).',
  'sasr.no_model': 'Henüz konuşma tanıma modeli yok. Model Hub’dan bir Whisper veya ONNX CTC modeli ekleyin ya da aşağıdan bir sherpa-onnx modeli indirin.',
  'sasr.recommended': 'Önerilen',
  'sasr.installed': 'Kurulu',
  'sasr.install': 'İndir',
  'sasr.installing': 'İndiriliyor...',
  'sasr.license': 'Lisans: {license}',
  'sasr.install_failed': 'Konuşma tanıma modeli indirilemedi: {error}',
  'sasr.model_installed': '“{name}” konuşma tanıma modeli hazır.',
  'sasr.only_transcribes': 'Bu model yalnızca yazıya çevirir; İngilizceye çeviri bir Whisper işidir.',
  'sasr.languages_more': '{list} ve {count} dil daha',
};

const en: Record<keyof typeof tr, string> = {
  'sasr.group_definitions': 'Library models',
  'sasr.group_sherpa': 'sherpa-onnx',
  'sasr.group_mlx': 'MLX (mlx-audio)',
  'sasr.group_python': 'Python Engine',
  'sasr.text_only': 'This model returns plain text; it makes no subtitles (SRT, WebVTT).',
  'sasr.kind': 'Speech recognition (sherpa-onnx)',
  'sasr.catalog_title': 'Recommended sherpa-onnx Recognition Models',
  'sasr.catalog_desc': 'ONNX models Whisper cannot run: NVIDIA Parakeet, SenseVoice and Omnilingual, which knows 1600 languages including Turkish. Long recordings are cut at pauses and every segment gets its time.',
  'sasr.no_runtime': 'The sherpa-onnx engine is not installed. It is the small tool that runs these models (about 20 MB, downloaded from the official release and checked with SHA-256).',
  'sasr.no_model': 'No speech recognition model yet. Add a Whisper or ONNX CTC model from Model Hub, or download a sherpa-onnx model below.',
  'sasr.recommended': 'Recommended',
  'sasr.installed': 'Installed',
  'sasr.install': 'Download',
  'sasr.installing': 'Downloading...',
  'sasr.license': 'License: {license}',
  'sasr.install_failed': 'The speech recognition model could not be downloaded: {error}',
  'sasr.model_installed': 'The speech recognition model “{name}” is ready.',
  'sasr.only_transcribes': 'This model only transcribes; translation to English is a Whisper task.',
  'sasr.languages_more': '{list} and {count} more',
};

export default { tr, en };
