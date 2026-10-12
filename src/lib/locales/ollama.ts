// Ollama-compatible API on the gateway (P2): Settings > Gateway section, Connections card and
// the backend keys of harness_core::gateway (Ollama port conflict / warnings).
const tr = {
  'ollama.allow_pull_help': 'Uyumlu istemcilerin model indirme isteğini kabul eder.',
  // Backend keys
  'gateway.ollama_port_conflict':
    'Ollama portu 1024 veya üstü olmalı; gateway ve Control API portlarından farklı olmalı.',
  'gateway.ollama_port_ollama':
    'Bu portta gerçek bir Ollama çalışıyor; Harness onu durdurmaz. Ollama uyumlu API yalnız gateway portunda. Portu kullanmak için Ollama\'yı kapatıp "Ollama portunda da dinle" anahtarını kapatıp yeniden açın ya da başka bir port seçin. Port',
  'gateway.ollama_port_harness':
    'Bu portta başka bir Harness gateway\'i (Ollama uyumlu) çalışıyor. Ollama uyumlu API yalnız bu gateway\'in portunda. Port',
  'gateway.ollama_port_busy':
    'Bu port başka bir program tarafından kullanılıyor; Ollama uyumlu API yalnız gateway portunda. Port',
  'gateway.ollama_port_installed':
    'Ollama bu bilgisayarda kurulu; Harness onun portunu almaz (yoksa Ollama sonradan açılamaz). Ollama uyumlu API yalnız gateway portunda. Ollama\'yı kaldırın ya da başka bir port seçin. Port',

  // Settings > Gateway
  'ollama.title': 'Ollama Uyumlu API',
  'ollama.desc':
    'Ollama istemcileri (ollama CLI, Python/JS kütüphaneleri, Open WebUI, editör eklentileri) gateway\'e Ollama gibi bağlanır: /api/chat, /api/generate, /api/embed, /api/tags, /api/ps, /api/show. İstekler aynı zamanlayıcıdan geçer (ilk istekte yükleme, boşta boşaltma, kuyruk) ve Analitik\'te görünür.',
  'ollama.address': 'Adres',
  'ollama.address_hint': 'İstemcide OLLAMA_HOST ya da "Ollama URL" olarak bu adresi girin.',
  'ollama.gateway_off': 'Gateway kapalı. Ollama uyumlu API gateway açıkken çalışır.',
  'ollama.port_enable': 'Ollama portunda da dinle',
  'ollama.port_enable_hint':
    'Yalnız Ollama\'nın varsayılan adresini bilen istemciler için. Varsayılan kapalı. Bu portta gerçek bir Ollama çalışıyorsa ya da Ollama kuruluysa Harness portu almaz, uyarı gösterir.',
  'ollama.port_pending': 'Beklemede',
  'ollama.port_pending_hint': 'Ayar açık ama port şu an dinlenmiyor (uyarıya bakın). Neden ortadan kalkınca anahtarı kapatıp yeniden açın.',
  'ollama.port': 'Ollama portu',
  'ollama.port_hint': 'Varsayılan 11434.',
  'ollama.allow_pull': '/api/pull ile indirmeye izin ver',
  'ollama.allow_pull_hint':
    'Yalnız Hugging Face adları indirilebilir: hf.co/<sahip>/<depo>:<kuant>. Ollama kayıt defteri adları (llama3.2 gibi) açık bir hatayla reddedilir. Varsayılan kapalı.',
  'ollama.names_title': 'Model Adları',
  'ollama.names_desc':
    'Ollama adı model tanımının adıdır (alias). Etiketsiz alias\'lar ":latest" ile listelenir ve "ad:latest" isteği "ad" tanımına gider. "qwen3:4b" gibi etiketli bir ad için tanımın adını "qwen3:4b" yapın. hf.co/<sahip>/<depo>:<kuant> adı, o depodan indirilmiş GGUF\'u kullanan tanıma gider.',
  'ollama.keep_alive_note':
    'keep_alive boşta boşaltma süresini o istek için değiştirir (0 = hemen boşalt, -1 = hiç boşaltma); keep_alive gönderilmezse tanımın kendi süresi geçerlidir.',

  // Connections card
  'ollama.card_title': 'Ollama Uyumlu Uç Nokta',
  'ollama.card_desc': 'Ollama istemcileri için aynı gateway: OLLAMA_HOST bu adres olmalı.',
  'ollama.card_off': 'Gateway açıkken kullanılabilir (Ayarlar > Gateway).',
  'ollama.card_port_badge': 'Ayrıca {port} portunda',
  'ollama.card_env': 'Ortam değişkeni',
};

const en: Record<keyof typeof tr, string> = {
  'ollama.allow_pull_help': 'Accepts model download requests from compatible clients.',
  'gateway.ollama_port_conflict':
    'The Ollama port must be 1024 or higher and differ from the gateway and Control API ports.',
  'gateway.ollama_port_ollama':
    'A real Ollama is running on this port; Harness does not stop it. The Ollama-compatible API stays on the gateway port only. To use the port, quit Ollama and turn "Also listen on the Ollama port" off and on again, or pick another port. Port',
  'gateway.ollama_port_harness':
    'Another Harness gateway (Ollama-compatible) is running on this port. The Ollama-compatible API stays on this gateway’s port only. Port',
  'gateway.ollama_port_busy':
    'Another program uses this port; the Ollama-compatible API stays on the gateway port only. Port',
  'gateway.ollama_port_installed':
    'Ollama is installed on this computer; Harness does not take its port (Ollama could not start later). The Ollama-compatible API stays on the gateway port only. Uninstall Ollama or pick another port. Port',

  'ollama.title': 'Ollama-Compatible API',
  'ollama.desc':
    'Ollama clients (the ollama CLI, the Python/JS libraries, Open WebUI, editor plugins) connect to the gateway as if it were Ollama: /api/chat, /api/generate, /api/embed, /api/tags, /api/ps, /api/show. Requests go through the same scheduler (load on first request, idle unload, queue) and show up in Analytics.',
  'ollama.address': 'Address',
  'ollama.address_hint': 'Enter this address as OLLAMA_HOST or the client’s "Ollama URL".',
  'ollama.gateway_off': 'The gateway is off. The Ollama-compatible API works while the gateway runs.',
  'ollama.port_enable': 'Also listen on the Ollama port',
  'ollama.port_enable_hint':
    'Only for clients that know nothing but Ollama’s default address. Off by default. If a real Ollama runs on that port, or Ollama is installed, Harness does not take the port and shows a warning.',
  'ollama.port_pending': 'Pending',
  'ollama.port_pending_hint': 'The setting is on but the port is not listened on right now (see the warning). Once the cause is gone, turn the switch off and on again.',
  'ollama.port': 'Ollama port',
  'ollama.port_hint': 'Default 11434.',
  'ollama.allow_pull': 'Allow downloads through /api/pull',
  'ollama.allow_pull_hint':
    'Only Hugging Face names can be pulled: hf.co/<owner>/<repo>:<quant>. Ollama registry names (like llama3.2) are refused with a clear error. Off by default.',
  'ollama.names_title': 'Model Names',
  'ollama.names_desc':
    'The Ollama name is the model definition’s name (alias). Aliases without a tag are listed with ":latest", and a request for "name:latest" goes to the "name" definition. For a tagged name like "qwen3:4b", name the definition "qwen3:4b". An hf.co/<owner>/<repo>:<quant> name goes to the definition that uses the GGUF downloaded from that repo.',
  'ollama.keep_alive_note':
    'keep_alive changes the idle unload time for that request (0 = unload right away, -1 = never); without keep_alive the definition’s own time applies.',

  'ollama.card_title': 'Ollama-Compatible Endpoint',
  'ollama.card_desc': 'The same gateway for Ollama clients: set OLLAMA_HOST to this address.',
  'ollama.card_off': 'Available while the gateway runs (Settings > Gateway).',
  'ollama.card_port_badge': 'Also on port {port}',
  'ollama.card_env': 'Environment variable',
};

export default { tr, en };
