// Interface audit (30.09): one server list, empty states for "no server running",
// the collapsed download history and the model badges moved into details.
const tr = {
  // Servers: saved servers and running instances in one list
  'ui.srv_intro': 'Kayıtlı sunucular ve şu an çalışanlar tek listede. Kayıtlı bir sunucu tek tıkla kendi portunda başlar.',
  'ui.srv_list_title': 'Tüm Sunucular',
  'ui.srv_col_server': 'Sunucu',
  'ui.srv_col_autostart': 'Açılışta Başlat',
  'ui.srv_unsaved': 'Kaydedilmedi',
  'ui.toast_close': 'Bildirimi kapat',
  'ui.srv_unsaved_tip': 'Bu sunucunun ayarları kayıtlı değil. Saklamak için Dashboard’da açıp “Sunucu Olarak Kaydet”e basın.',
  'ui.srv_save_in_dashboard': 'Dashboard’da Aç ve Kaydet',
  'ui.srv_empty_title': 'Henüz sunucu yok',
  'ui.srv_empty_desc': 'Dashboard’da bir model seçip başlatın ya da “Yeni Sunucu” ile ayrı bir sunucu oluşturun.',

  // "No server running" empty state (Dashboard telemetry, Performance)
  'ui.off_title': '{name} çalışmıyor',
  'ui.off_failed_title': 'Sunucu bir hatayla durdu',
  'ui.off_failed_desc': 'Hatayı giderip sunucuyu yeniden başlatın.',
  'ui.off_failed_dash_desc': 'Hata ve kurtarma seçenekleri “Sunucuyu Başlat” düğmesinin üstünde; ayarları düzeltip yeniden başlatın.',
  'ui.off_starting_title': 'Sunucu başlatılıyor',
  'ui.off_starting_desc': 'Model belleğe yükleniyor. Ölçümler sunucu hazır olunca burada görünür.',
  'ui.off_start': 'Sunucuyu Başlat',
  'ui.off_open_dashboard': 'Dashboard’da Başlat',
  'ui.off_dash_desc': 'Başlattığınızda üretim hızı, bağlam önbelleği ve slotlar burada canlı görünür.',
  'ui.off_dash_desc_left': '“Model ve Model Motoru”ndan bir model seçip “Sunucuyu Başlat”a basın; üretim hızı, bağlam önbelleği ve slotlar burada canlı görünür.',
  'ui.off_perf_desc': 'Performans ölçümleri sunucu çalışırken toplanır. Başlattığınızda üretim hızı, bellek, slotlar ve istekler burada görünür.',
  'ui.tp_live': 'Canlı',
  'ui.perf_avg_peak': 'ort. {avg} · tepe {peak} t/s',
  'ui.perf_vram_of': '{total} içinden · tepe {peak}',
  'ui.perf_kv': 'Bağlam önbelleği %{pct} dolu',
  'ui.chart_waiting': 'İlk ölçüm bekleniyor',

  // Model Hub download queue
  'ui.dl_completed': '{count} indirme tamamlandı',

  // Library: secondary facts live under "Details"
  'ui.lib_details': 'Ayrıntılar',
  'ui.lib_source': 'Kaynak',
  'ui.lib_preset': 'Son preset',
  'ui.lib_parts': 'Parçalar',
  'ui.lib_added': 'Eklenme',

  // Dashboard fit verdict details
  'ui.fit_details': 'Bellek Ayrıntıları',
  'ui.fit_layers': 'GPU katmanı',
  'ui.fit_kv': 'Bağlam önbelleği',
  'ui.fit_draft': 'Taslak model',
  'ui.fit_source': 'Kaynak',

  // Dashboard controls
  'ui.dash_server': 'Sunucu',
  'ui.dash_saved': 'Kayıtlı',
};

const en: Record<keyof typeof tr, string> = {
  'ui.srv_intro': 'Saved servers and the ones running right now, in one list. A saved server starts on its own port with one click.',
  'ui.srv_list_title': 'All Servers',
  'ui.srv_col_server': 'Server',
  'ui.srv_col_autostart': 'Start at Launch',
  'ui.srv_unsaved': 'Not saved',
  'ui.toast_close': 'Close notification',
  'ui.srv_unsaved_tip': 'This server’s settings are not saved. To keep them, open it in the Dashboard and click “Save as Server”.',
  'ui.srv_save_in_dashboard': 'Open in Dashboard to Save',
  'ui.srv_empty_title': 'No servers yet',
  'ui.srv_empty_desc': 'Pick a model on the Dashboard and start it, or create a separate one with “New Server”.',

  'ui.off_title': '{name} is not running',
  'ui.off_failed_title': 'The server stopped with an error',
  'ui.off_failed_desc': 'Fix the error and start the server again.',
  'ui.off_failed_dash_desc': 'The error and recovery options are above “Start Server”; fix the settings and start again.',
  'ui.off_starting_title': 'Starting the server',
  'ui.off_starting_desc': 'The model is loading into memory. Measurements appear here once the server is ready.',
  'ui.off_start': 'Start Server',
  'ui.off_open_dashboard': 'Start from the Dashboard',
  'ui.off_dash_desc': 'Once it runs, generation speed, context cache and slots show up here live.',
  'ui.off_dash_desc_left': 'Pick a model under “Model and Runtime” and click “Start Server”; generation speed, context cache and slots show up here live.',
  'ui.off_perf_desc': 'Performance is measured while a server runs. Once it starts, generation speed, memory, slots and requests show up here.',
  'ui.tp_live': 'Live',
  'ui.perf_avg_peak': 'avg {avg} · peak {peak} t/s',
  'ui.perf_vram_of': 'of {total} · peak {peak}',
  'ui.perf_kv': 'Context cache {pct}% full',
  'ui.chart_waiting': 'Waiting for the first sample',

  'ui.dl_completed': '{count} completed',

  'ui.lib_details': 'Details',
  'ui.lib_source': 'Source',
  'ui.lib_preset': 'Last preset',
  'ui.lib_parts': 'Parts',
  'ui.lib_added': 'Added',

  'ui.fit_details': 'Memory Details',
  'ui.fit_layers': 'GPU layers',
  'ui.fit_kv': 'Context cache',
  'ui.fit_draft': 'Draft model',
  'ui.fit_source': 'Source',

  'ui.dash_server': 'Server',
  'ui.dash_saved': 'Saved',
};

export default { tr, en };
