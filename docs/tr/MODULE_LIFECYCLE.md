# Modül Yaşam Döngüsü

[English](../en/MODULE_LIFECYCLE.md)

0.2 W3'ün ilk dilimi Konuşma Tanıma ve Hermes Agent modüllerini kapsar. Modüller ekranı kurulu modül/bileşen sürümlerini, ölçülen çalışma zamanı alanını, ayrı sayılan paylaşılan alanı, tahminleri, sahipleri ve kurtarma işlemlerini gösterir. Etkinleştir/Devre Dışı Bırak dosyaları ve sahipliği korur. Devre dışı modül konuşma çalışanı başlatamaz, ajana bağlanamaz veya görev çalıştıramaz. Çalışma zamanını değiştirmeden önce süren görevleri ve arka uçları durdurun; ajan görevinin Durdur denetimi kullanılabilir kalır.

Güncelle, birlikte gelen modül sürümü ilerlediğinde veya bileşen sabiti kayıtlı kurulumdan farklı olduğunda görünür. Bu, otomatik upstream sürüm taraması değil, uygulamayla gelen uyumluluk sözleşmesidir. Listeleme sürüm kayıtlarını değiştirmez. Hermes kaynak sürümü [v0.22.2](https://github.com/NousResearch/hermes-agent/releases/tag/v0.22.2), `d923a5eb487a17c0a68801bba9ef0f56c81040a8` commit'idir. Harici kurulumlar kullanılabilir; Güncelle harici kaynak kurulumunu değiştirmeden Harness tarafından yönetilen kopyayı yükler.

## Kurulum ve Kurtarma

Her yaşam döngüsü işlemi bildirim sözleşmesini, platformu ve en düşük uygulama sürümünü denetler. Yükle/Güncelle yeni kurulumun tamamı, indirmeler ve ek pay için boş alan denetimi yapar; eski çalışma zamanı geri dönüş yedeği olarak diskte kalır. Onar önce süre sınırlı çalışma zamanı doğrulaması yapar. Sağlıklı kurulum çevrimdışı doğrulanabilir; bozuk kurulum yeniden yüklenip doğrulanır.

Değiştirme, eski çalışma zamanını yanındaki `.rollback` klasörüne taşımadan önce kurtarma kaydı yazar. Kurulum son konumda yapılır; Python ortamının mutlak yolları korunur. Hata/iptal önceki çalışma zamanını, modül durumunu, sürümleri ve sahipliği geri yükler. Kesinti sonrası listeleme kaydı yeniden uygular. Geri yükleme başarısızsa kayıt/yedek yeniden denemek için korunur. Paylaşılan Python/uv başka motorlarca da kullanıldığı için kaldırmada korunur. Onar, uv ve yönetilen Python içe aktarımlarını doğrular; bozuk destek klasörleri aynı kurtarma kaydına alınırken sağlıklı Python sürümleri korunur. Başka modül sahibi veya çalışan Python arka ucu varsa destek değiştirme reddedilir; ortam kurulumuyla eş zamanlı değiştirme yapılmaz. Başka sahibi bulunan bileşen silinmez; uyumsuz paylaşılan sürüm değişimi reddedilir.

Ağ ön denetimi çalışan kurulumu taşımadan önce yerelleştirilmiş bağlantı/yeniden deneme hatası verir. Sonraki paket/ağ hataları da geri alınır. Kaldır, Etkinleştir ve Devre Dışı Bırak indirme gerektirmez. Disk işlemleri, sorgular ve kurulum masaüstü arayüz yürütücüsünün dışında çalışır. Konuşma başlatma yaşam döngüsü değişimleriyle sıraya alınır. Süreç grupları durdurma, zaman aşımı ve kurulum tamamlanmasında sonlandırılıp toplanır.

Hermes, resmî kurucunun prerequisites/repository/venv/python-deps/complete aşamalarını `<veri>/bin/modules/hermes` altında ayrı kurulum evi, Python ve önbellek konumlarıyla kullanır. Genel komut yayımlama, ürün/arayüz derlemeleri, kabuk profili değişiklikleri, kurulum sihirbazı ve servisler atlanır. Kurucu betiği resmî HTTPS adresinden alınır ve kullanıcı yetkileriyle çalışır; kaynak sabiti kurucu betiğinin hash ile sabitlendiği anlamına gelmez. Yayından önce temiz kurulum ve platform bazında uçtan uca doğrulama gerekir.

## Kaldırma ve Kullanıcı Verileri

Kaldırma modelleri, veri kümelerini, projeleri ve değerli çıktıları korur. Bunlar modül bileşeni değil, paylaşılan kullanıcı kaynaklarıdır; silme işlemlerini Kitaplıkta veya özgün klasörlerinde ayrı yönetin. Kitaplıkta model kaydını kaldırmak ve fiziksel dosyaları silmek ayrı seçimlerdir.

Kaldırma onayı varsayılan olarak tüm kullanıcı verilerini korur. Hermes, ölçülen boyutlarıyla ayrı ve işaretsiz **Ajan Geçmişini de Sil** ve **Ajan Günlüklerini de Sil** seçenekleri sunar. Geçmiş, algılanan Hermes evindeki `sessions`, `state.db` ve SQLite yan dosyalarını; günlükler `logs` klasörünü kapsar. Açıkça seçilirse aynı evdeki diğer Hermes arayüzlerinin geçmişi de silinir. Yapılandırmalar, gizli bilgiler, beceriler, bellekler, projeler ve çıktılar korunur. Harici Hermes kurulumları korunur. Çalışma zamanını kullanan süreç kaldırmayı; harici çalışma zamanını kullanan süreç de seçilen geçmiş/günlük silme işlemini engeller.

NSIS kancaları yalnızca Harness PATH girdisini temizler; uygulama/kullanıcı verisini silmez. Olağan uygulama kaldırma veri kökünü ve harici/özel klasörleri korur; kurucunun veri silme seçeneği ayrıca açıkça seçilmelidir. MSI, macOS uygulama silme, AppImage silme ve `.deb` paket kaldırma kullanıcı evindeki verileri silme izni sayılmaz. Kancalar için hedefli koruma testi vardır; bu dilim her platformda kurucu çalıştırıldığını iddia etmez.

## Doğrulama

Hedefli Rust testleri paylaşılan sahipliği, disk hesabını, kesintili değiştirmeyi, kurtarma kaydını, devre dışı durum/sürüm geri yüklemesini ve ilk kurulum temizliğinde kullanıcı dosyalarının korunmasını kapsar. İndirmeli konuşma entegrasyon testi isteğe bağlıdır. Arayüz işlemi/silme varsayılanları `scripts/test-modules.mjs` içinde denetlenir.
