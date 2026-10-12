# Araçlar

English: [../en/06-tools.md](../en/06-tools.md)

**Bu sayfada ne yapılır:** Modelin çağırabileceği araçları seçer, izinleri ayarlar ve gerektiğinde yeni araçlar ürettirirsiniz.

## Yerleşik Araçlar

Araç, modelin metin üretmek dışında yaptığı bir işlemdir. Sol menüdeki **Oluştur** bölümünde (Gelişmiş düzeyde bir bölüm) bulunan **Araçlarım** sayfası, tüm kaynaklardaki araçları gösterir. Bir sohbette hangilerinin kullanılacağını Playground'da seçersiniz ([Playground > Araçları Kullan ve Onayları Gözden Geçir](04-playground.md#araçları-kullan-ve-onayları-gözden-geçir)); Basit düzeyde mesaj kutusundaki **Web ve Araçlar** anahtarı, web aramasını ve seçtiğiniz araçları modele sunar.

| İşlev | Araç adları |
| --- | --- |
| Dosya okuma ve arama | `read_file`, `file_glob_search`, `grep_search`, `get_info` |
| Dosya yazma ve düzenleme | `write_file`, `edit_file` |
| Komut çalıştırma | `exec_shell_command` |
| Tarih ve hesap | `current_datetime`, `calculator` |
| Ağ, arama ve hava durumu | `fetch_url`, `web_search`, `geocode`, `weather` |

Dosya ve komut araçları bir çalışma klasörü gerektirir. Ağ araçları dış servislere istek gönderir. **Ayarlar > Araçlar > Yerleşik Araçlar** içindeki **Web Araması** bölümünden sağlayıcı seçebilirsiniz. Anahtarsız arama engellenebilir; SearXNG, Brave Search ve Tavily seçenekleri de vardır.

## İzin Politikasını Seç

1. **Ayarlar > Araçlar > Yerleşik Araçlar** sayfasını (Gelişmiş bir sayfa) açın.
2. **İzin Politikası** seçimini inceleyin.
3. Dosya araçları için bir **Varsayılan klasör** belirleyin. Playground sohbetleri kendi çalışma klasörünü de seçebilir.
4. Sohbette bir çağrı geldiğinde adını ve parametrelerini okuyup sonra onaylayın.

| Politika | Davranış |
| --- | --- |
| **Kapalı** | Araçlar çalışmaz. |
| **Her seferinde sor** | Güvenli araçlar çalışır; dosya okuma, yazma ve komut çağrıları onay ister. |
| **Güvenilir araçlara izin ver** | Güvenli araçlar ve güvenilir olarak işaretlediğiniz araçlar sormadan çalışır; diğerleri onay ister. |
| **Hepsine izin ver** | Dosya yazma ve komutlar dahil tüm araçlar onaysız çalışır. |

Bir model kendi izinlerini genişletemez. Komut çalıştırmak, yalnızca dosya okumaktan daha geniş etki yaratır; bir komutu onaylamadan önce okuyun. İzin sınırlarının ayrıntıları [güvenlik modelindedir](../../tr/SECURITY_MODEL.md).

## Araç Oluşturucu

1. Sol menüde **Oluştur** bölümünü, ardından **Araç Oluşturucu** sayfasını açın ve model uç noktasını seçin. Çalışan bir sunucu ya da açık bir ağ geçidi gerekir.
2. **Araç ne yapmalı?** alanına küçük, somut bir iş yazın. Örnek: "Verilen metindeki kelimeleri say ve en sık geçen üç kelimeyi döndür."
3. Gerekirse **İzin verilen alan adları**, **Okunabilir klasörler** ve **Yazılabilir klasörler** alanlarını doldurun. Boş bırakılan alan, ilgili erişimi kapatır. Klasörler çalışma klasörüne göre yazılır.
4. **Oluştur** düğmesine basın. Model kod ve test üretir; Harness Studio testi bir kum havuzunda çalıştırır.
5. **Kod (index.js)**, **Manifest**, izinler ve test sonucunu inceleyin. Testi geçemeyen araç onaylanamaz.
6. **Onayla, Bu Oturumda Kullan** ile aracı etkinleştirin. Oturum aracı, uygulama kapanınca kalkar. Saklamak için **Kalıcı Kaydet** düğmesini kullanın.

Üretilen JavaScript araçları yalnızca paket izinlerinde yazılı ağ ve dosya alanlarına erişebilir; komut çalıştıramaz. Bu sınır, yerleşik `exec_shell_command` aracından farklıdır. Kurulumu onaylamak, sonraki çağrılar için izin politikasını ortadan kaldırmaz.

## Otomatik Araç Oluşturucu

1. **Oluştur** bölümünde **Otomatik Araç Oluşturucu** sayfasını açın.
2. Bir **Model** ve **Planlanacak araç sayısı** seçin, ardından **Otomatik Araç Oluştur** düğmesine basın.
3. **Üretim Sırası** içindeki işleri sürükleyerek ya da **Öne al** ve **Geri Al** ile yeniden sıralayın.
4. Gerekirse **Duraklat**, **Devam Et** veya **Durdur** kullanın. Sıra ve sonuçlar saklanır.
5. Ağ ya da dosya erişimi isteyen paketler **İzin bekliyor** durumunda kalır. Birini **Paketi İncele** ile inceleyin ve kabul ediyorsanız **Bu İzinlerle Kur** seçeneğini seçin.

Ağ ve dosya erişimi gerektirmeyen araçlar otomatik kurulur. Test sırasında dış erişim kapalıdır. Başarısız üretimlerde hata ayrıntısını okuyun ve **Başarısızları Tekrar Dene** seçeneğini kullanın.

## Kurulu Araçlar ve MCP

**Ayarlar > Araçlar > Kurulu Araçlar** sayfasında paket etkinliğini yönetebilir, **Testi Çalıştır** ve **Kaldır** işlemlerini kullanabilirsiniz. Hazır bir paket için **Klasörden Kur…** vardır; izinleri okuyup **İzinleri Onayla ve Kur** seçeneğini seçin.

MCP sunucularının araçları **Bağlantılar > MCP Sunucuları** altından eklenir. Yerel komut kullanan bir MCP sunucusu, kullanıcı yetkilerinizle program çalıştırır. Harness Studio'nun kendi araçlarını başka bir MCP istemcisine açmak ayrı bir ayardır; [CLI, Control API ve MCP](09-cli-api-mcp.md) bölümüne bakın.

Paket biçimi, testler ve üretimin nasıl çalıştığı için [Araçlar başvurusunu](../../tr/TOOLS.md) kullanın. Bir model araçları doğru çağırmıyorsa [araç tanılama notlarına](../../tr/TOOL-DIAGNOSIS.md) bakın.

Sıradaki bölüm: [Performans ve Analitik](07-performans-ve-analitik.md).
