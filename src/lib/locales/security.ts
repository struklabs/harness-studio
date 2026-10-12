// Network & security: Settings section, exposure confirmation, endpoint labels, runtime
// integrity confirmation, and the error keys the backend returns (harness_core::network,
// harness_core::runtime::ERR_INTEGRITY_UNVERIFIED).
const tr = {
  // Backend error keys
  'network.lan_without_key':
    'Sunucu ağa açık bir adrese bağlanacak ama API anahtarı yok. Bir anahtar ayarlayın, adresi 127.0.0.1 yapın ya da bu sunucu için "Yine de Aç" onayı verin.',
  'network.tools_without_key':
    'Araçlar/MCP açıkken sunucu anahtarsız ağa açılamaz (model dosya ve süreç erişimi kazanır). Bir API anahtarı ayarlayın veya adresi 127.0.0.1 yapın.',
  'network.invalid_host':
    'Bağlanma adresi geçersiz: yalnızca bir IP adresi (ör. 127.0.0.1, 0.0.0.0, 192.168.1.20) veya localhost kullanılabilir.',
  'network.invalid_origin':
    'Geçersiz origin: http(s)://alan[:port], localhost veya * yazın (yol, virgül ve boşluk olmadan).',
  'network.key_generation_failed': 'Güçlü anahtar üretilemedi.',
  'runtime.integrity_unverified': 'Bu model motoru arşivinin SHA-256 özeti yok; kurmak için açık onayınız gerekiyor.',

  // Settings section
  'settings.tab_security': 'Ağ ve Güvenlik',
  'settings.desc_security': 'API anahtarları ve sunucuları ağa anahtarsız açma onayları.',
  'sec.bind_title': 'Varsayılan Bağlanma Adresi',
  'sec.bind_desc':
    'Kendi adresi olmayan her sunucu (Kontrol paneli oturumu ve adres belirtilmemiş profiller) bu adresi kullanır. Sunucuya özel adres, sunucunun kendi ayarlarında kalır.',
  'sec.bind_loopback': 'Yalnız bu bilgisayar',
  'sec.bind_loopback_desc': '127.0.0.1: başka cihazlar bağlanamaz. Önerilen.',
  'sec.bind_lan': 'Yerel ağ',
  'sec.bind_lan_desc': '0.0.0.0: aynı ağdaki cihazlar bağlanabilir. API anahtarı gerekir.',
  'sec.bind_custom': 'Özel adres',
  'sec.bind_custom_desc': 'Tek bir ağ arayüzünün IP adresi (ör. 192.168.1.20).',
  'sec.bind_custom_placeholder': '192.168.1.20',
  'sec.bind_invalid': 'Geçerli bir IP adresi yazın.',
  'sec.lan_addresses_title': 'Diğer cihazlar şu adreslerden bağlanır',
  'sec.lan_addresses_none': 'Etkin bir yerel ağ adresi bulunamadı.',
  'sec.firewall_note':
    'Windows Güvenlik Duvarı ilk başlatmada llama-server için izin isteyebilir. Harness Studio güvenlik duvarı ayarlarını değiştirmez; yalnızca "Özel ağlar" için izin vermeniz önerilir.',
  'sec.lan_key_note': 'Yerel ağa açık bir sunucu API anahtarı olmadan başlatılmaz.',

  'sec.keys_title': 'API Anahtarları',
  'sec.keys_desc': 'Anahtarlar yalnızca işletim sistemi anahtarlığında tutulur; dosyalara yazılmaz.',
  'sec.keys_default_server': 'Kontrol paneli sunucusu',
  'sec.key_set': 'Anahtar var',
  'sec.key_unset': 'Anahtar yok',
  'sec.key_running_with': 'Çalışıyor, anahtarla',
  'sec.key_running_without': 'Çalışıyor, anahtarsız',
  'sec.key_set_in_dashboard': 'Kontrol Panelinde Ayarla',
  'sec.key_generate': 'Güçlü Anahtar Üret',
  'sec.key_regenerate': 'Yeni Anahtar Üret',
  'sec.key_copy': 'Anahtarı Kopyala',
  'sec.key_generated_toast': 'Yeni anahtar anahtarlığa kaydedildi; sunucu yeniden başlatılınca geçerli olur.',
  'sec.key_copied_toast': 'Anahtar panoya kopyalandı.',
  'sec.key_regenerate_confirm':
    'Mevcut anahtar geçersiz olacak; bu sunucuya bağlanan istemcileri güncellemeniz gerekir. Devam edilsin mi?',
  'sec.key_generate_failed': 'Anahtar üretilemedi: {error}',
  'sec.keys_none': 'Kayıtlı sunucu yok.',
  'sec.host_label': 'Adres',

  'sec.cors_title': "İzinli Tarayıcı Origin'leri (CORS)",
  'sec.cors_desc':
    "Tarayıcıda çalışan hangi web uygulamalarının sunuculara istek atabileceği (--cors-origins). Masaüstü istemciler ve komut satırı araçları CORS'tan etkilenmez.",
  'sec.cors_placeholder': 'http://localhost:3000\nhttps://app.example.com',
  'sec.cors_default_loopback':
    "Boş bırakılırsa: yalnız bu bilgisayardaki sunucularda llama-server varsayılanı (tüm origin'ler; araçlar/MCP açıkken yalnız localhost).",
  'sec.cors_default_lan': "Boş bırakılırsa: ağa açık sunucularda hiçbir tarayıcı origin'ine izin verilmez.",
  'sec.cors_special': 'Özel değerler: localhost (yalnız localhost sayfaları), * (tümü).',
  'sec.cors_multi_warning':
    'Bu llama-server sürümü listeyi olduğu gibi tek başlıkta gönderir; tarayıcılar yalnızca tek bir origin yazıldığında kabul eder.',
  'sec.cors_unsupported': 'Kullanılan model motoru --cors-origins bayrağını desteklemiyor; liste uygulanmaz.',
  'sec.cors_multi_gateway':
    "Birden çok origin gerekiyorsa Gateway'i (yönetilen uç nokta) kullanın: isteğin Origin'i listedeyse yalnız onu geri gönderir. Gateway'in listesi Ayarlar > Gateway'dedir.",
  'sec.cors_supported': 'Model motoru destekliyor',
  'sec.cors_unknown': 'Model motoru desteği bilinmiyor',

  'sec.acks_title': '"Yine de Aç" Onayları',
  'sec.acks_desc':
    'Bu sunucular API anahtarı olmadan ağa açılabilir. Onayı kaldırınca bir sonraki başlatma yeniden engellenir.',
  'sec.acks_none': 'Onay yok: anahtarsız ağ erişimi engelli.',
  'sec.ack_at': 'Onay: {date} · {host}',
  'sec.ack_revoke': 'Onayı Kaldır',
  'sec.ack_revoked_toast': 'Onay kaldırıldı.',

  // Exposure confirmation dialog
  'sec.dialog_title': 'Sunucu anahtarsız olarak ağa açılacak',
  'sec.dialog_body':
    '"{server}" sunucusu {host} adresine bağlanacak ve API anahtarı yok. Aynı ağdaki herkes modeli kullanabilir ve sunucunun kaynaklarını tüketebilir.',
  'sec.dialog_hint': 'Önerilen: bir API anahtarı ayarlayın veya adresi 127.0.0.1 yapın.',
  'sec.dialog_open_anyway': 'Yine de Aç',
  'sec.dialog_settings': 'Ağ ve Güvenlik Ayarları',
  'sec.dialog_cancel': 'Vazgeç',
  'sec.dialog_failed': 'Başlatılamadı: {error}',

  // Persistent warning
  'sec.banner_unprotected': 'Anahtarsız ağ erişimi açık',
  'sec.banner_unprotected_desc': '{servers}: API anahtarı olmadan ağa açık. Ağdaki herkes erişebilir.',

  // Endpoints (Connections)
  'sec.ep_direct_title': 'Doğrudan llama-server Uç Noktası',
  'sec.ep_direct_desc':
    "İstemciler llama-server'a doğrudan bağlanır. Bu yolda istek analitiği, istemci bilgisi, kuyruk ve modelin istek gelince yüklenmesi (JIT) yoktur; yalnızca /metrics toplamları görülür.",
  'sec.ep_managed_title': 'Yönetilen uç nokta (Gateway)',
  'sec.ep_managed_desc': 'Tek adres, çok model, istek başına kayıt ve JIT yükleme.',
  'sec.ep_soon': 'Yakında',
  'sec.ep_active': 'Etkin',
  'sec.scope_loopback': 'Yalnız bu bilgisayar',
  'sec.scope_lan': 'Yerel ağ',
  'sec.auth_key': 'API anahtarı var',
  'sec.auth_none': 'Anahtarsız',
  'sec.ep_lan_urls': 'Diğer cihazlar için:',

  // Runtime integrity confirmation
  'sec.integrity_title': 'Doğrulanamayan Model Motoru Arşivi',
  'sec.integrity_body':
    'GitHub bu arşivler için SHA-256 özeti yayımlamıyor; indirilen dosyanın değiştirilmediği doğrulanamaz. Yalnızca kaynağa güveniyorsanız devam edin.',
  'sec.integrity_col_asset': 'Arşiv',
  'sec.integrity_col_size': 'Boyut',
  'sec.integrity_reason_missing': 'özet yok',
  'sec.integrity_reason_invalid': 'geçersiz özet',
  'sec.integrity_install': 'Doğrulamadan Kur',
  'sec.integrity_cancel': 'Vazgeç',
  'sec.integrity_cancelled': 'Kurulum iptal edildi: arşiv doğrulanamadı.',
  'runtime.backend_no_device':
    'Bu build uyumlu bir GPU görmedi (kart desteklenmiyor veya sürücü ya da CUDA/ROCm/oneAPI eksik ya da uyumsuz olabilir). Mevcut model motoru değişmedi; Vulkan build’ini kurabilirsiniz.',
  'runtime.backend_missing_libs': 'Eksik dosyalar: {libs}.',
  'runtime.backend_missing_hip': 'Eksik dosyalar: {libs}. Windows’ta ROCm build’i AMD HIP SDK’nin kurulu olmasını ister.',
};

const en: Record<keyof typeof tr, string> = {
  'network.lan_without_key':
    'The server would bind to a network address without an API key. Set a key, use 127.0.0.1, or confirm "Open Anyway" for this server.',
  'network.tools_without_key':
    'With tools/MCP enabled a server can’t be exposed to the network without a key (the model gets file and process access). Set an API key or use 127.0.0.1.',
  'network.invalid_host':
    'Invalid bind address: only an IP address (e.g. 127.0.0.1, 0.0.0.0, 192.168.1.20) or localhost is allowed.',
  'network.invalid_origin': 'Invalid origin: use http(s)://host[:port], localhost or * (no path, comma or spaces).',
  'network.key_generation_failed': 'Could not generate a strong key.',
  'runtime.integrity_unverified':
    'This runtime archive has no SHA-256 digest; installing it needs your explicit confirmation.',

  'settings.tab_security': 'Network & Security',
  'settings.desc_security': 'API keys and confirmations for opening servers to the network without a key.',
  'sec.bind_title': 'Default Bind Address',
  'sec.bind_desc':
    'Every server without an address of its own (the Dashboard session and profiles without one) uses this address. A server-specific address stays in that server’s settings.',
  'sec.bind_loopback': 'This computer only',
  'sec.bind_loopback_desc': '127.0.0.1: other devices can’t connect. Recommended.',
  'sec.bind_lan': 'Local network',
  'sec.bind_lan_desc': '0.0.0.0: devices on the same network can connect. Requires an API key.',
  'sec.bind_custom': 'Custom address',
  'sec.bind_custom_desc': 'The IP address of a single network interface (e.g. 192.168.1.20).',
  'sec.bind_custom_placeholder': '192.168.1.20',
  'sec.bind_invalid': 'Enter a valid IP address.',
  'sec.lan_addresses_title': 'Other devices connect via',
  'sec.lan_addresses_none': 'No active local network address found.',
  'sec.firewall_note':
    'Windows Firewall may ask to allow llama-server on first start. Harness Studio never changes firewall settings; allowing it for "Private networks" only is recommended.',
  'sec.lan_key_note': 'A server exposed to the local network doesn’t start without an API key.',

  'sec.keys_title': 'API Keys',
  'sec.keys_desc': 'Keys live only in the OS keychain; they are never written to files.',
  'sec.keys_default_server': 'Dashboard server',
  'sec.key_set': 'Key set',
  'sec.key_unset': 'No key',
  'sec.key_running_with': 'Running, with key',
  'sec.key_running_without': 'Running, without key',
  'sec.key_set_in_dashboard': 'Set in Dashboard',
  'sec.key_generate': 'Generate Strong Key',
  'sec.key_regenerate': 'Generate New Key',
  'sec.key_copy': 'Copy Key',
  'sec.key_generated_toast': 'New key saved to the keychain; it takes effect when the server restarts.',
  'sec.key_copied_toast': 'Key copied to the clipboard.',
  'sec.key_regenerate_confirm': 'The current key will stop working; clients of this server must be updated. Continue?',
  'sec.key_generate_failed': 'Could not generate the key: {error}',
  'sec.keys_none': 'No saved servers.',
  'sec.host_label': 'Address',

  'sec.cors_title': 'Allowed Browser Origins (CORS)',
  'sec.cors_desc':
    'Which web apps running in a browser may call the servers (--cors-origins). Desktop clients and command-line tools are not affected by CORS.',
  'sec.cors_placeholder': 'http://localhost:3000\nhttps://app.example.com',
  'sec.cors_default_loopback':
    'If empty: servers on this computer only keep the llama-server default (all origins; localhost only while tools/MCP are on).',
  'sec.cors_default_lan': 'If empty: servers exposed to the network allow no browser origin.',
  'sec.cors_special': 'Special values: localhost (localhost pages only), * (all).',
  'sec.cors_multi_warning':
    'This llama-server build sends the list as is in one header; browsers only accept it when a single origin is listed.',
  'sec.cors_unsupported': 'The runtime in use has no --cors-origins flag; the list is not applied.',
  'sec.cors_multi_gateway':
    'If you need several origins, use the gateway (managed endpoint): it sends back the request\'s Origin only when it is on the list. The gateway has its own list in Settings > Gateway.',
  'sec.cors_supported': 'Supported by runtime',
  'sec.cors_unknown': 'Runtime support unknown',

  'sec.acks_title': '"Open Anyway" Confirmations',
  'sec.acks_desc':
    'These servers may be exposed to the network without an API key. Revoking blocks the next start again.',
  'sec.acks_none': 'No confirmations: keyless network access is blocked.',
  'sec.ack_at': 'Confirmed: {date} · {host}',
  'sec.ack_revoke': 'Revoke',
  'sec.ack_revoked_toast': 'Confirmation revoked.',

  'sec.dialog_title': 'The server would be exposed without a key',
  'sec.dialog_body':
    'Server "{server}" would bind to {host} without an API key. Anyone on the same network could use the model and its resources.',
  'sec.dialog_hint': 'Recommended: set an API key or use 127.0.0.1.',
  'sec.dialog_open_anyway': 'Open Anyway',
  'sec.dialog_settings': 'Network & Security Settings',
  'sec.dialog_cancel': 'Cancel',
  'sec.dialog_failed': 'Could not start: {error}',

  'sec.banner_unprotected': 'Keyless network access on',
  'sec.banner_unprotected_desc':
    '{servers}: exposed to the network without an API key. Anyone on the network can use it.',

  'sec.ep_direct_title': 'Direct llama-server Endpoint',
  'sec.ep_direct_desc':
    'Clients talk to llama-server directly. This path has no request analytics, client attribution, queueing or load-on-request (JIT); only /metrics totals are visible.',
  'sec.ep_managed_title': 'Managed endpoint (Gateway)',
  'sec.ep_managed_desc': 'One address, many models, per-request records and JIT loading.',
  'sec.ep_soon': 'Coming soon',
  'sec.ep_active': 'Active',
  'sec.scope_loopback': 'This computer only',
  'sec.scope_lan': 'Local network',
  'sec.auth_key': 'API key set',
  'sec.auth_none': 'No key',
  'sec.ep_lan_urls': 'For other devices:',

  'sec.integrity_title': 'Unverifiable Runtime Archive',
  'sec.integrity_body':
    'GitHub publishes no SHA-256 digest for these archives, so the download can’t be verified as untampered. Continue only if you trust the source.',
  'sec.integrity_col_asset': 'Archive',
  'sec.integrity_col_size': 'Size',
  'sec.integrity_reason_missing': 'no digest',
  'sec.integrity_reason_invalid': 'invalid digest',
  'sec.integrity_install': 'Install Unverified',
  'sec.integrity_cancel': 'Cancel',
  'sec.integrity_cancelled': 'Installation cancelled: the archive could not be verified.',
  'runtime.backend_no_device':
    'This build detected no compatible GPU (the card may be unsupported, or the driver or CUDA/ROCm/oneAPI may be missing or incompatible). The current runtime is unchanged; you can install the Vulkan build.',
  'runtime.backend_missing_libs': 'Missing files: {libs}.',
  'runtime.backend_missing_hip': 'Missing files: {libs}. On Windows the ROCm build needs the AMD HIP SDK installed.',
};

export default { tr, en };
