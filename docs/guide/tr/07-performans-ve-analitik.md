# Performans ve Analitik

English: [../en/07-performance-and-analytics.md](../en/07-performance-and-analytics.md)

**Bu sayfada ne yapılır:** Model hızını ölçer, kaynak kullanımını izler ve ağ geçidi isteklerinin gecikme ya da hata nedenini araştırırsınız.

**Performans** ve **Analitik** sayfaları sol menüdeki **İzle** grubundadır. Bunlar Gelişmiş düzey sayfalarıdır; görmüyorsanız **Gelişmiş** düzeye geçin ([Basit ve Gelişmiş Arayüz Düzeyleri](README.md#basit-ve-gelişmiş-arayüz-düzeyleri)).

## Canlı Performansı İzle

1. Bir sunucu başlatın ve birkaç sohbet mesajı gönderin.
2. Sol menüde **İzle** altındaki **Performans** sayfasını açın ve üstteki seçiciden sunucuyu seçin.
3. **Canlı** grubundaki **Özet**, **Üretim Hızı ve Gecikme**, **Bellek ve GPU** ve **İstemci Dağılımı** sayfalarına bakın.
4. Aynı modeli farklı yükleme ayarlarıyla denerken hız ve bellek kullanımını karşılaştırın.

`t/s` cinsinden üretim hızı, saniyede üretilen token sayısıdır. İstem işleme hızı, gönderdiğiniz içeriğin ne kadar hızlı okunduğudur. İlk token süresi yanıtın ne zaman başladığını, toplam süre ise isteğin tamamını anlatır. Performans ekranındaki yaklaşık ilk token süresi, sunucu bunu ayrı bildirmediğinde istem işleme süresinden alınır.

GPU yükü ve sıcaklığı NVIDIA'da `nvidia-smi` ile, Linux'ta AMD ve Intel için sistem sensörlerinden okunur (Intel'de yalnızca sıcaklık); Windows ve macOS'ta AMD ve Intel için bu değerler görünmeyebilir. Eksik sensör verisini sıfır kullanım diye yorumlamayın. Sunucunun RAM kullanımı ile uygulamanın RAM kullanımı ayrı gösterilebilir.

## Benchmark Çalıştır

1. **Performans > Benchmark > Yeni Ölçüm** sayfasını açın.
2. Yerel bir **Model** seçin. İlk denemede **Hızlı** hazır ayarını kullanabilirsiniz.
3. **İstem boyutları (pp)** ve **Üretim boyutları (tg)** alanlarını kontrol edin. Birden fazla değer, farklı test kombinasyonları oluşturur.
4. Karşılaştırmalar için **GPU Katman Taraması**, **Flash-attn Karşılaştırma** veya **KV Cache Karşılaştırma** hazır ayarını seçebilirsiniz.
5. Başka bir sunucu çalışıyorsa önce onu durdurmayı düşünün; GPU'yu paylaşmak ölçümü düşürür.
6. **Benchmark'ı Başlat** düğmesine basın. Sonucu **Sonuçlar** bölümünden okuyun. Gerekirse **İptal Et** kullanın.
7. **Karşılaştır** akışında en az iki kayıt seçin. Modelin, model motorunun, parametrelerin ve test boyutlarının karşılaştırmaya uygun olduğunu kontrol edin.

Benchmark için `llama-bench` gerekir. Bulunamıyorsa [sorun giderme tablosuna](10-sorun-giderme.md#model-motoru-motor-eksik-veya-başlamıyor) bakın. Benchmark sabit bir test yüküdür; gerçek bir sohbetin hızını birebir garanti etmez. En son benchmark'ın en iyi istem ve üretim hızları, ağ geçidinin gerçek kullanım ortalamalarının yanında gösterilir; böylece ikisini karşılaştırabilirsiniz.

## Analitik ve İstek Günlüğü

1. Sol menüde **İzle** altındaki **Analitik** sayfasını açın. Ağ geçidinin açık olduğundan ve **Gateway isteklerini kaydet** seçiminin etkin olduğundan emin olun (**Kayıt ve Saklama Politikası** bölümündedir).
2. **Genel Bakış** ve **Model ve İstemci Kırılımı** ile istekleri, tokenları, hızı, gecikmeyi ve hata oranlarını inceleyin.
3. **Günlük** sayfasında **Model**, **İstemci**, **Uç Nokta** ve **Durum** filtrelerini kullanın.
4. Bir istek seçin. **Seçili İstek Ayrıntısı** altındaki **Zaman Çizelgesi**, **Token ve Hız**, **Kaynaklar** ve **Hata** bölümlerini okuyun. Zaman çizelgesi kuyruk, yükleme, istem ve üretim aşamalarını gösterir; başarısız bir araç çağrısı için olası suçlu (sunucu ya da şablon, bağlam, model veya istemci) teşhis edilir.
5. Kuyruk ya da yükleme süreleri yüksekse modelin ilk kez yüklenip yüklenmediğine ve başka isteklerin çalışıp çalışmadığına bakın.

P50 ortanca gecikmeyi, P95 ise isteklerin büyük çoğunluğunun altında tamamlandığı gecikmeyi gösterir. Bu değerler ham kayıtların tutulduğu dönemden hesaplanır; daha eski günlük toplamlardan aynı ayrıntıyı elde edemezsiniz.

Ağ geçidi istekleri tek tek kaydedilir. Doğrudan Playground ve araç oluşturucu kullanımı toplam token dağılımına eklenebilir; doğrudan çağrıların aynı istek ayrıntısı yoktur. Diğer doğrudan kullanım için metriklerden çıkarılan toplam yaklaşık olabilir.

**Kayıt ve Saklama Politikası** sayfasında ham kayıtların ne kadar süre tutulacağını düzenleyebilirsiniz (varsayılan 30 gündür; daha eski satırlar günlük toplamlara dönüştürülür). Varsayılan olarak kaydedilen üst veriler zaman, model, istemci, token, süre, bellek ve hata bilgisidir. İstem ve yanıt içeriği varsayılan olarak kaydedilmez.

Bir model tanımında **İstek içeriğini kaydet (analitik)** açılırsa ağ geçidinden geçen istemler ve yanıtlar diske düz metin olarak yazılır. Hassas içerik için bunu açmayın. **Analitiği Sil** geri alınamaz; sohbetler bu işlemden etkilenmez.

**Araç Çağrıları** günlüğünde araç adını, istemciyi, sonucu ve onay durumunu inceleyebilirsiniz. Reddedilen çağrılar da kaydedilir.

Ölçüm koşulları için [Features](../../tr/FEATURES.md) ve [Features](../../tr/FEATURES.md), özellik kapsamı için [Özellikler başvurusuna](../../tr/FEATURES.md) bakın.

Sıradaki bölüm: [Ayarlar](08-ayarlar.md).
