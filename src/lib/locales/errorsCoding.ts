// Backend error keys of the Coding area (harness_core::coding).
// Texts are single quoted literals (the CLI compiles the English half in); {name} = parameter.
const tr = {
  'coding.folder_missing': 'Proje klasörü bulunamadı. Klasörü yeniden bağlamak için projeyi düzenleyin.',
  'coding.invalid_id': 'Geçersiz tanımlayıcı: {id}',
  'coding.io_failed': 'Dosya işlemi başarısız: {path} ({error})',
  'coding.checkpoint_not_found': 'Denetim noktası bulunamadı: {id}',
  'coding.git_failed': 'Git çalıştırılamadı: {error}',
  'coding.snapshot_too_large': 'Klasör anlık görüntü için çok büyük (en fazla {files} dosya ve {megabytes} MB). Klasörü Git deposu yapın veya daha küçük bir klasör seçin.',
  'coding.outside_workspace': 'Yol çalışma alanının dışında: {path}',
  'coding.patch_invalid': 'Yama okunamadı: {error}',
  'coding.patch_path': 'Yama bu yola yazamaz: {path} ({error})',
  'coding.patch_binary': 'Yama bir metin dosyasına uygulanmalı: {path}',
  'coding.patch_failed': 'Yama uygulanamadı: {error}',
  'coding.empty_prompt': 'Görev boş olamaz.',
};

const en: Record<keyof typeof tr, string> = {
  'coding.folder_missing': 'The project folder was not found. Edit the project to point it at the folder again.',
  'coding.invalid_id': 'Invalid identifier: {id}',
  'coding.io_failed': 'File operation failed: {path} ({error})',
  'coding.checkpoint_not_found': 'Checkpoint not found: {id}',
  'coding.git_failed': 'Could not run Git: {error}',
  'coding.snapshot_too_large': 'The folder is too large for a snapshot (at most {files} files and {megabytes} MB). Make the folder a Git repository or choose a smaller folder.',
  'coding.outside_workspace': 'The path is outside the workspace: {path}',
  'coding.patch_invalid': 'The patch could not be read: {error}',
  'coding.patch_path': 'The patch cannot write to this path: {path} ({error})',
  'coding.patch_binary': 'A patch can only be applied to a text file: {path}',
  'coding.patch_failed': 'The patch could not be applied: {error}',
  'coding.empty_prompt': 'The task cannot be empty.',
};

export default { tr, en };
