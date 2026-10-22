// Backend error keys of device pairing (harness_core::pairing, control API /pairing, /devices).
// Texts are single quoted literals (the CLI compiles the English half in).
const tr = {
  'pairing.invalid_code': 'Eşleştirme kodu yanlış, kullanılmış ya da süresi dolmuş.',
  'pairing.locked': 'Çok fazla yanlış deneme. Bir dakika bekleyin.',
  'pairing.expired': 'Eşleştirme isteğinin süresi doldu.',
  'pairing.not_found': 'Eşleştirme isteği bulunamadı.',
  'pairing.device_not_found': 'Cihaz bulunamadı.',
  'pairing.device_name_invalid': 'Cihaz adı boş olamaz (en fazla 48 karakter).',
  'pairing.too_many': 'Çok fazla açık kod ya da bekleyen istek var. Birkaç dakika sonra deneyin.',
  'pairing.admin_ack_required': 'Yönetici erişimi için onay kutusunu işaretleyin.',
  'pairing.not_pending': 'Bu istek artık beklemiyor.',
  'pairing.nonce_invalid': 'İstemci değeri geçersiz.',
  'pairing.unreachable': 'Uzak Harness’a ulaşılamadı.',
  'pairing.bad_answer': 'Uzak Harness beklenmeyen bir yanıt verdi.',
  'pairing.not_approved': 'Eşleşme henüz onaylanmadı.',
  'pairing.not_a_device': 'Yalnızca eşleşmiş bir cihazın kendi token’ı bildirim yapabilir.',
  'pairing.failed': 'Eşleştirme başarısız oldu.',
  'pairing.signaling_url_invalid': 'Boş bırakın ya da kimlik bilgisi, sorgu ve parça içermeyen bir HTTP(S) adresi girin.',
};

const en: Record<keyof typeof tr, string> = {
  'pairing.invalid_code': 'The pairing code is wrong, used or expired.',
  'pairing.locked': 'Too many wrong tries. Wait a minute.',
  'pairing.expired': 'The pairing request expired.',
  'pairing.not_found': 'The pairing request was not found.',
  'pairing.device_not_found': 'Device not found.',
  'pairing.device_name_invalid': 'The device name cannot be empty (48 characters at most).',
  'pairing.too_many': 'Too many open codes or pending requests. Try again in a few minutes.',
  'pairing.admin_ack_required': 'Tick the confirmation box for admin access.',
  'pairing.not_pending': 'This request is no longer waiting.',
  'pairing.nonce_invalid': 'The client value is invalid.',
  'pairing.unreachable': 'The remote Harness could not be reached.',
  'pairing.bad_answer': 'The remote Harness gave an unexpected answer.',
  'pairing.not_approved': 'The pairing has not been approved yet.',
  'pairing.not_a_device': 'Only a paired device’s own token can report.',
  'pairing.failed': 'Pairing failed.',
  'pairing.signaling_url_invalid': 'Leave it empty or enter an HTTP(S) address without credentials, query or fragment.',
};

export default { tr, en };
