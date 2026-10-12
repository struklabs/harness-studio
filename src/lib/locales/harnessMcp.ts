// Connections > Tools and clients > "Harness MCP'yi ekle": registers the Harness MCP
// server (`harness mcp` / the Control API's /mcp) in Claude Code, Codex, OpenCode and Cursor.
const tr = {
  'hmcp.token_risk': 'Token aracın yapılandırma dosyasına düz metin olarak yazılır.',
  'hmcp.token_hint': 'Ayarlar > Geliştirici API bölümünden gerekli izinlerle bir token oluşturun.',
  'hmcp.title': "Harness MCP'yi Ekle",
  'hmcp.desc':
    'Kodlama ajanları Harness araçlarını kullansın: modelleri listeleme, yükleme/boşaltma, uç nokta alma, metrik, log ve benchmark.',
  'hmcp.mode_label': 'Bağlantı',
  'hmcp.mode_stdio': 'Komut (stdio, Önerilen)',
  'hmcp.mode_http': 'HTTP (Kontrol API)',
  'hmcp.command_label': 'harness komutu',
  'hmcp.command_missing':
    'harness komut satırı aracı bulunamadı (uygulamayla birlikte gelir; geliştirme sürümünde: cargo build -p harness-cli). PATH üzerine koyabilir ya da yolunu buraya yazabilirsiniz.',
  'hmcp.models_write': 'İndirme ve model silmeye izin ver (models:write)',
  'hmcp.stdio_note':
    'Araç, harness mcp komutunu kendisi başlatır. Araçlar Harness Studio (Geliştirici API açık) ya da harness serve üzerinden çalışır; yapılandırmaya sır yazılmaz.',
  'hmcp.url_label': 'MCP adresi',
  'hmcp.api_off': 'Kontrol API kapalı. Ayarlar > Geliştirici API bölümünden açın.',
  'hmcp.token_label': 'İsimli token',
  'hmcp.token_placeholder': 'hs_…',
  'hmcp.token_required': 'HTTP bağlantısı için isimli token gerekli.',
  'hmcp.configured': 'Ekli',
  'hmcp.add': 'Ekle',
  'hmcp.update': 'Güncelle',
  'hmcp.remove': 'Kaldır',
  'hmcp.removing': 'Kaldırılıyor…',
  'hmcp.added': 'Harness MCP eklendi. Aracı yeniden başlatın.',
  'hmcp.removed': 'Harness MCP kaydı kaldırıldı.',
  'hmcp.config_error': 'Yapılandırma okunamadı: {message}',
  'hmcp.preview_title': "{name}: Harness MCP Kaydı",
  'hmcp.preview_desc':
    'Yalnız "harness" kaydı eklenir ya da değiştirilir; dosyanın geri kalanı olduğu gibi kalır. Yazmadan önce bir yedek alınır.',
  'hmcp.preview_large': 'Dosya büyük; yalnız eklenecek kayıt gösteriliyor.',
  'hmcp.preview_backup': 'Yedek:',
  'hmcp.manual_title': 'Elle ekleme',
  'hmcp.docs': 'Belgeler',
  'hmcp.claude_restart_note':
    'Claude Code ~/.claude.json dosyasını kendisi de yazar; eklemeden sonra açık oturumları yeniden başlatın.',
} as const;

const en: Record<keyof typeof tr, string> = {
  'hmcp.token_risk': 'The token is written to the tool’s configuration file in plain text.',
  'hmcp.token_hint': 'Create a token with the required permissions in Settings > Developer API.',
  'hmcp.title': 'Add Harness MCP',
  'hmcp.desc':
    'Let coding agents use Harness tools: list models, load/unload, get endpoints, metrics, logs and benchmarks.',
  'hmcp.mode_label': 'Connection',
  'hmcp.mode_stdio': 'Command (stdio, Recommended)',
  'hmcp.mode_http': 'HTTP (Control API)',
  'hmcp.command_label': 'harness command',
  'hmcp.command_missing':
    'The harness command-line tool was not found (it ships with the app; in a development build: cargo build -p harness-cli). Put it on PATH or enter its path here.',
  'hmcp.models_write': 'Allow downloads and model deletion (models:write)',
  'hmcp.stdio_note':
    'The tool starts harness mcp itself. Tools work through Harness Studio (Developer API on) or harness serve; no secret is written to the config.',
  'hmcp.url_label': 'MCP URL',
  'hmcp.api_off': 'The Control API is off. Turn it on in Settings > Developer API.',
  'hmcp.token_label': 'Named token',
  'hmcp.token_placeholder': 'hs_…',
  'hmcp.token_required': 'An HTTP connection needs a named token.',
  'hmcp.configured': 'Added',
  'hmcp.add': 'Add',
  'hmcp.update': 'Update',
  'hmcp.remove': 'Remove',
  'hmcp.removing': 'Removing…',
  'hmcp.added': 'Harness MCP added. Restart the tool.',
  'hmcp.removed': 'Harness MCP entry removed.',
  'hmcp.config_error': 'Could not read the config: {message}',
  'hmcp.preview_title': '{name}: Harness MCP Entry',
  'hmcp.preview_desc':
    'Only the "harness" entry is added or changed; the rest of the file stays as it is. A backup is made before writing.',
  'hmcp.preview_large': 'The file is large; only the entry being written is shown.',
  'hmcp.preview_backup': 'Backup:',
  'hmcp.manual_title': 'Add manually',
  'hmcp.docs': 'Docs',
  'hmcp.claude_restart_note':
    'Claude Code writes ~/.claude.json itself too; restart open sessions after adding.',
};

export default { tr, en };
