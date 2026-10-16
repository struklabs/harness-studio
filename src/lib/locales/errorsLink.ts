// Backend error keys of Harness Link (harness_core::link, gateway remote proxy).
// Texts are single quoted literals (the CLI compiles the English half in); {name} = parameter.
const tr = {
  'link.unreachable': 'Uzak Harness’a ulaşılamıyor.',
  'link.unauthorized': 'Uzak Harness token’ı reddedildi. Inference kapsamını ve model izinlerini kontrol edin.',
  'link.timeout': 'Uzak Harness bağlantısı zaman aşımına uğradı.',
  'link.certificate_invalid': 'Sertifika doğrulanamadı.',
  'link.chained_not_allowed': 'Uzak Harness istekleri başka bir uzak Harness’a zincirlenemez.',
  'link.inference_scope_warning': 'Bu token tam yetkili (admin). Inference kapsamlı bir token kullanın.',
  'link.tls_failed': 'Uzak Harness TLS bağlantısı kurulamadı.',
  'link.dns_failed': 'Uzak Harness adresi çözümlenemedi.',
  'link.unsupported_endpoint': 'Bu uç nokta uzak modeller için desteklenmiyor.',
  'link.insecure_not_allowed': 'HTTP yalnızca açık izinle, yerel veya özel ağ adreslerinde kullanılabilir. Genel internet için HTTPS kullanın.',
  'link.name_taken': 'Bu bağlantı adı zaten kullanılıyor.',
  'link.invalid_name': 'Geçerli bir bağlantı adı girin: küçük Latin harfleri, rakam ve kısa çizgi; en fazla 64 karakter.',
  'link.invalid_url': 'Kimlik bilgisi, sorgu veya parça içermeyen bir HTTP ya da HTTPS adresi girin.',
  'link.not_found': 'Uzak Harness bağlantısı bulunamadı.',
  'link.keychain_unavailable': 'Sistem anahtar zincirine erişilemedi. Token kaydedilmedi.',
  'link.token_required_for_new_address': "Adres değiştiği için token'ı yeniden girin.",
  'link.removed_token_left': 'Bağlantı kaldırıldı ancak sistem anahtar zincirine erişilemediği için kayıtlı token silinemedi.',
};

const en: Record<keyof typeof tr, string> = {
  'link.unreachable': 'The remote Harness is unreachable.',
  'link.unauthorized': 'The remote Harness token was rejected. Check its inference scope and model permissions.',
  'link.timeout': 'The remote Harness connection timed out.',
  'link.certificate_invalid': 'Certificate could not be verified.',
  'link.chained_not_allowed': 'Remote Harness requests cannot be chained to another remote Harness.',
  'link.inference_scope_warning': 'This token has full (admin) access. Use a token with the inference scope.',
  'link.tls_failed': 'The remote Harness TLS connection failed.',
  'link.dns_failed': 'The remote Harness address could not be resolved.',
  'link.unsupported_endpoint': 'This endpoint is unsupported for remote models.',
  'link.insecure_not_allowed': 'HTTP requires explicit consent and a local or private network address. Use HTTPS over the public internet.',
  'link.name_taken': 'This connection name is already in use.',
  'link.invalid_name': 'Enter a valid connection name: lowercase Latin letters, digits and hyphens; up to 64 characters.',
  'link.invalid_url': 'Enter an HTTP or HTTPS address without credentials, query or fragment.',
  'link.not_found': 'The remote Harness connection was not found.',
  'link.keychain_unavailable': 'The system keychain is unavailable. The token was not saved.',
  'link.token_required_for_new_address': 'Re-enter the token because the address changed.',
  'link.removed_token_left': 'The connection was removed, but the saved token could not be deleted because the system keychain is unavailable.',
};

export default { tr, en };
