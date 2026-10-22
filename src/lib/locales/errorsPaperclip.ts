// Backend error keys of Paperclip (harness_core::paperclip, connection to the user's install).
// Texts are single quoted literals (the CLI compiles the English half in); {name} = parameter.
const tr = {
  'paperclip.url_invalid': 'Kimlik bilgisi, sorgu veya parça içermeyen bir HTTPS adresi girin. HTTP yalnızca bu makine için kabul edilir (localhost).',
  'paperclip.not_configured': 'Paperclip adresi kayıtlı değil. Önce Ajanlar bölümünde bağlanın.',
  'paperclip.no_company': 'Bu Paperclip kurulumunda henüz şirket yok. Önce Paperclip’te bir şirket oluşturun.',
  'paperclip.company_required': 'Bu kurulumda birden fazla şirket var. Gösterilecek şirketi seçin.',
  'paperclip.unauthorized': 'Paperclip kimlik doğrulamasını reddetti. Yönetim (board) API anahtarını kontrol edin; yerel güvenilir kurulumda anahtar gerekmez.',
  'paperclip.forbidden': 'Paperclip isteğe izin vermedi. Anahtarın bu şirkete erişimi olduğunu kontrol edin.',
  'paperclip.api_missing': 'Bu adreste Paperclip API’si veya istenen kayıt bulunamadı. Adresi kontrol edin (varsayılan http://localhost:3100).',
  'paperclip.redirect': 'Paperclip adresi başka bir yere yönlendiriyor. Son adresi (örneğin https ile) girin.',
  'paperclip.server_error': 'Paperclip sunucusu hata verdi (HTTP {status}).',
  'paperclip.http_error': 'Paperclip beklenmeyen bir yanıt verdi (HTTP {status}).',
  'paperclip.unreachable': 'Paperclip’e bağlanılamadı ({url}). Paperclip’in bu adreste çalıştığını kontrol edin.',
  'paperclip.invalid_response': 'Yanıt bir Paperclip API yanıtına benzemiyor. Adresin Paperclip’e ait olduğunu kontrol edin.',
  'paperclip.id_invalid': 'Geçersiz kimlik.',
  'paperclip.too_many_links': 'Çok fazla görev bağlantısı var. Kullanmadıklarınızı kaldırın.',
  'paperclip.keychain_unavailable': 'Sistem anahtar zincirine erişilemedi: {error}',
};

const en: Record<keyof typeof tr, string> = {
  'paperclip.url_invalid': 'Enter an HTTPS address without credentials, query or fragment. HTTP is only accepted for this machine (localhost).',
  'paperclip.not_configured': 'No Paperclip address is saved. Connect first on the Agents page.',
  'paperclip.no_company': 'This Paperclip install has no company yet. Create one in Paperclip first.',
  'paperclip.company_required': 'This install has more than one company. Choose the company to show.',
  'paperclip.unauthorized': 'Paperclip rejected the credentials. Check the board API key; a local trusted install needs no key.',
  'paperclip.forbidden': 'Paperclip refused the request. Check that the key can access this company.',
  'paperclip.api_missing': 'The Paperclip API or the requested record was not found at this address. Check the address (default http://localhost:3100).',
  'paperclip.redirect': 'The Paperclip address redirects elsewhere. Enter the final address (for example with https).',
  'paperclip.server_error': 'The Paperclip server returned an error (HTTP {status}).',
  'paperclip.http_error': 'Paperclip returned an unexpected answer (HTTP {status}).',
  'paperclip.unreachable': 'Could not connect to Paperclip ({url}). Check that Paperclip is running at this address.',
  'paperclip.invalid_response': 'The answer does not look like a Paperclip API response. Check that the address belongs to Paperclip.',
  'paperclip.id_invalid': 'Invalid id.',
  'paperclip.too_many_links': 'There are too many task links. Remove the ones you no longer use.',
  'paperclip.keychain_unavailable': 'The system keychain is unavailable: {error}',
};

export default { tr, en };
