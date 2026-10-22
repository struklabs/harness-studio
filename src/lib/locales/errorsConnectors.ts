// Backend error keys of the OAuth connectors (harness_core::connectors, secrets).
// Texts are single quoted literals (the CLI compiles the English half in); {name} = parameter.
const tr = {
  'connector.keychain_unavailable': 'Sistem anahtar zincirine erişilemiyor; bağlayıcı belirteci okunamadı veya kaydedilemedi.',
  'connector.consent_required': 'Bağlanmadan önce onay kutusunu işaretleyin.',
  'connector.unknown': 'Bilinmeyen bağlayıcı: {id}.',
  'connector.client_id_missing': '{name} için OAuth istemci kimliği yok. Ayarlar > Araçlar > Bağlayıcılar bölümünden ekleyin.',
  'connector.not_connected': '{name} bağlı değil. Ayarlar > Araçlar > Bağlayıcılar bölümünden bağlanın.',
  'connector.expired': '{name} oturumu sona erdi. Bağlayıcıyı yeniden bağlayın.',
};

const en: Record<keyof typeof tr, string> = {
  'connector.keychain_unavailable': 'The system keychain is unavailable; the connector token could not be read or saved.',
  'connector.consent_required': 'Tick the consent box before connecting.',
  'connector.unknown': 'Unknown connector: {id}.',
  'connector.client_id_missing': 'There is no OAuth client ID for {name}. Add one in Settings > Tools > Connectors.',
  'connector.not_connected': '{name} is not connected. Connect it in Settings > Tools > Connectors.',
  'connector.expired': 'The {name} sign-in expired. Connect the connector again.',
};

export default { tr, en };
