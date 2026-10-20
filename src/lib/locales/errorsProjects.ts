// Backend error keys of projects (harness_core::projects, state projects).
// Texts are single quoted literals (the CLI compiles the English half in); {name} = parameter.
const tr = {
  'projects.not_found': 'Proje bulunamadı.',
  'projects.invalid_name': 'Proje adı boş olamaz.',
  'projects.archived': 'Proje arşivlendi; yeni sohbet, çıktı veya görev eklenemez.',
  'projects.folder_invalid': 'Klasör kullanılamıyor: {path}. Var olan bir klasörün tam yolunu girin.',
  'projects.too_many_folders': 'En fazla {max} ek klasör eklenebilir.',
  'projects.outside_folders': 'Bu konum projenin klasörlerinin dışında: {path}',
  'projects.no_folder': 'Projenin klasörü yok. Proje ayarlarından bir klasör seçin.',
  'projects.write_failed': 'Dosya yazılamadı: {path} ({error})',
  'projects.output_missing': 'Dosya bulunamadı: {path}',
  'projects.output_not_found': 'Çıktı kaydı bulunamadı.',
  'projects.export_failed': 'Proje dışa aktarılamadı: {path} ({error})',
  'projects.invalid_data': 'Kaydedilecek dosya verisi geçersiz.',
  'projects.task_not_found': 'Ajan görevi bulunamadı veya çıktısı yok.',
};

const en: Record<keyof typeof tr, string> = {
  'projects.not_found': 'Project not found.',
  'projects.invalid_name': 'The project name cannot be empty.',
  'projects.archived': 'The project is archived; no new chats, outputs or tasks can be added.',
  'projects.folder_invalid': 'This folder cannot be used: {path}. Enter the full path of an existing folder.',
  'projects.too_many_folders': 'At most {max} extra folders can be added.',
  'projects.outside_folders': 'This location is outside the project folders: {path}',
  'projects.no_folder': 'The project has no folder. Choose one in the project settings.',
  'projects.write_failed': 'Could not write the file: {path} ({error})',
  'projects.output_missing': 'File not found: {path}',
  'projects.output_not_found': 'Output record not found.',
  'projects.export_failed': 'Could not export the project: {path} ({error})',
  'projects.invalid_data': 'The file data to save is invalid.',
  'projects.task_not_found': 'The agent task was not found or has no output.',
};

export default { tr, en };
