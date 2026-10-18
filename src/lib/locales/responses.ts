// Stateful Responses on the gateway (P2): Settings > Gateway section.
const tr = {
  'responses.title': 'Durumlu Responses',
  'responses.desc':
    '/v1/responses yanıtlarını bu bilgisayarda (state.db) saklar; istemci previous_response_id ile konuşmayı sürdürür, geçmişi yeniden göndermez. Saklanan yanıtlar onu oluşturan token\'a aittir; başka bir token okuyamaz ve sürdüremez. Önceki yanıtın instructions alanı taşınmaz.',
  'responses.store': 'Yanıtları Sakla',
  'responses.store_hint':
    'Açıkken store: false göndermeyen istekler saklanır (OpenAI varsayılanı). Kapatınca yeni yanıt saklanmaz; saklanmış olanlar süresi dolana ya da silinene kadar okunabilir.',
  'responses.retention_days': 'Saklama Süresi (gün)',
  'responses.retention_days_hint': 'Oluşturulduktan ya da son sürdürüldükten bu kadar gün sonra silinir.',
  'responses.max_stored': 'En Çok Yanıt',
  'responses.max_stored_hint': 'Sınır aşılınca en uzun süredir kullanılmayanlar silinir.',
  'responses.max_mib': 'En Çok Alan (MiB)',
  'responses.max_mib_hint': 'Girdi ve yanıt içeriğinin toplam boyutu.',
  'responses.compact_auto': 'Uzun Zincirleri Otomatik Özetle',
  'responses.compact_auto_hint': 'Açıkken eşiği aşan her zincirli istekte eski halkalar, modelin yazdığı bir özetle değiştirilir (özet yanıtla birlikte saklanır; sonraki turlar ondan başlar). Kapalıyken yalnız `context_management` isteyen istekler özetlenir.',
  'responses.compact_percent': 'Özet Eşiği (% Bağlam)',
  'responses.compact_percent_hint': 'Konuşma, modelin slot başına bağlam penceresinin bu kadarını doldurunca özetlenir. İstek `compact_threshold` (token) ile kendi eşiğini verebilir.',
  'responses.compact_keep': 'Olduğu Gibi Kalan Son Halka',
  'responses.compact_keep_hint': 'En yeni bu kadar halka (istek + yanıt) özetlenmez, aynen modele gider.',
  'responses.stored': 'Saklanan Yanıtlar',
  'responses.stored_value': '{count} yanıt, {size}',
  'responses.clear': 'Tümünü Sil',
  'responses.clear_title': 'Saklanan yanıtlar silinsin mi?',
  'responses.clear_desc': '{count} yanıt kalıcı olarak silinir; onlara `previous_response_id` ile devam eden istemciler 404 alır. Geri alınamaz.',
  'responses.clear_cancel': 'Vazgeç',
  'responses.cleared': 'Saklanan yanıtlar silindi.',
};

const en: Record<keyof typeof tr, string> = {
  'responses.title': 'Stateful Responses',
  'responses.desc':
    'Keeps /v1/responses answers on this computer (state.db) so a client continues a conversation with previous_response_id instead of resending the history. A stored response belongs to the token that created it; another token cannot read or continue it. The instructions of the previous response are not carried over.',
  'responses.store': 'Store Responses',
  'responses.store_hint':
    'When on, requests that do not send store: false are stored (OpenAI\'s default). When off, nothing new is stored; stored responses stay readable until they expire or are deleted.',
  'responses.retention_days': 'Keep for (days)',
  'responses.retention_days_hint': 'Deleted this many days after creation or the last continuation.',
  'responses.max_stored': 'Most Responses',
  'responses.max_stored_hint': 'Above the limit, the least recently used are deleted.',
  'responses.max_mib': 'Most Space (MiB)',
  'responses.max_mib_hint': 'Total size of the stored input and response content.',
  'responses.compact_auto': 'Summarize Long Chains Automatically',
  'responses.compact_auto_hint': 'When on, every chained request over the threshold has its older links replaced by a summary the model writes (the summary is stored with the response; later turns start from it). When off, only requests that send `context_management` are summarized.',
  'responses.compact_percent': 'Summary Threshold (% of Context)',
  'responses.compact_percent_hint': 'The conversation is summarized once it fills this much of the model’s per-slot context window. A request can set its own threshold with `compact_threshold` (tokens).',
  'responses.compact_keep': 'Newest Links Kept as They Are',
  'responses.compact_keep_hint': 'This many of the newest links (request + response) are not summarized; they go to the model verbatim.',
  'responses.stored': 'Stored Responses',
  'responses.stored_value': '{count} responses, {size}',
  'responses.clear': 'Delete All',
  'responses.clear_title': 'Delete stored responses?',
  'responses.clear_desc': '{count} responses are deleted permanently; clients continuing them with `previous_response_id` get 404. This cannot be undone.',
  'responses.clear_cancel': 'Cancel',
  'responses.cleared': 'Stored responses deleted.',
};

export default { tr, en };
