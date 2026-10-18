# CLI, Control API ve MCP

English: [../en/09-cli-api-mcp.md](../en/09-cli-api-mcp.md)

**Bu sayfada ne yapılır:** Terminal aracının, yönetim API'sinin ve MCP'nin ne işe yaradığını anlar ve ayrıntılı başvurulara ulaşırsınız.

Sohbet etmek, model indirmek ya da sunucu başlatmak için terminale ihtiyacınız yoktur. Bu bölüm, başka bir programın Harness Studio'yu kullanmasını isteyenler içindir.

| Yol | Ne sağlar? | Ayrıntı |
| --- | --- | --- |
| CLI (`harness`) | Terminalden model, sunucu, profil, günlük ve benchmark yönetimi; arayüzsüz (headless) sohbet ve çalıştırma. Konuşma, görsel, GGUF dönüştürme, LoRA ölçekleri, yönlendirici (router) modu ve analitiği de komut satırından kapsar. | [CLI başvurusu](../../tr/CLI.md) |
| Control API | Başka programların Harness Studio'nun modellerini ve sunucularını yönetmesini sağlar. Modellerle konuştuğunuz ağ geçidinden ayrı bir hizmettir. | [Güvenlik modeli](../../tr/SECURITY_MODEL.md), [CLI başvurusu](../../tr/CLI.md) |
| Harness MCP sunucusu | Claude Code, Codex, OpenCode veya Cursor gibi istemcilerin araçlarla modelleri listelemesini, yüklemesini ve metrikleri okumasını sağlar. | [MCP başvurusu](../../tr/MCP.md) |

Aşağıdaki Geliştirici ayarları ve MCP bağlantı sayfaları **Gelişmiş** düzeydedir. **Basit / Gelişmiş** ile geçiş yapın veya sayfayı komut paletinden açın; arayüz düzeyi CLI veya API izinlerini değiştirmez.

## Control API'yi Aç

1. **Ayarlar > Geliştirici > Control API** sayfasını açın.
2. **API'yi etkinleştir** seçimini açıp **Uygula** düğmesine basın.
3. **Temel Adres** alanını kontrol edin. Varsayılan `http://127.0.0.1:11500/api/harness/v1` adresidir; port doluysa görünen adres farklı olabilir.
4. API'nin tanımını görmek için **OpenAPI Şeması** alanındaki **Aç** düğmesini kullanın. Varsayılan OpenAPI şema adresi `http://127.0.0.1:11500/api/harness/v1/openapi.json` yalnızca bu bilgisayarda ve API açıkken anlamlıdır; port değiştiyse uygulamadaki bağlantıyı kullanın.

Yönetim istekleri bir tokenla korunur. Yerel `/health` ve şema erişimi için istisnalar ile token kapsamları [güvenlik modelinde](../../tr/SECURITY_MODEL.md) açıklanır. Uygulamanın veri klasörünü yönetirken CLI ona bu API üzerinden ulaşır; API kapalıysa CLI sahibi olan örneğe bağlanamaz.

**CLI** sayfasında (**Ayarlar > Geliştirici > CLI**) **Terminalde Kullan** komut satırı aracını gösterir, **PATH'e Ekle** ise `harness` komutunu bir terminalden kullanılabilir yapar; ardından yeni bir terminal açın. CLI çıktısı İngilizcedir.

## API'yi Koddan Kullan

Depoda Control API için hazır istemciler var: [`sdk/`](../../../sdk/README.md) altında bir TypeScript paketi ve bir Python paketi. OpenAPI şemasından üretildikleri için her uç noktanın, parametrenin ve yanıtın türü vardır. TypeScript için `npm install @struklabs/harness-studio-sdk` kullanın; Python paketini depo klasöründen kurun (PyPI yayını planlanıyor).

İkisi de çalışan Harness Studio'yu kendisi bulur: uygulama ya da `harness serve`, adresini veri klasöründeki `instance.json` dosyasına yazar; oturum anahtarı da yanındaki `api-token` dosyasındadır. Başka bir bilgisayar için adresi ve adlandırılmış bir anahtarı kendiniz verin.

```python
from harness_studio import connect, unwrap
from harness_studio_api.api.system import get_status

status = unwrap(get_status.sync_detailed(client=connect()))
print(status.version)
```

Hata yanıtı bir istisnaya dönüşür (Python'da `HarnessError`, TypeScript'te `HarnessApiError`). İstisna, uygulamanın gösterdiği `code` değerini taşır; örneğin anahtarın yetkisi yetmediğinde `api.insufficient_scope`.

## MCP'nin İki Yönü

Harness Studio bir **MCP sunucusu** olduğunda başka ajanlar yönetim araçlarını çağırır. `harness mcp` yerel komut yoludur; Control API ayrıca bir HTTP MCP uç noktası da sunar. Kurulum ve kapsamlar için [MCP başvurusunu](../../tr/MCP.md) izleyin. Ayrıca **Bağlantılar > Araçlar ve İstemciler > Harness MCP'yi Ekle** sayfasından bir istemciye ekleyebilirsiniz.

Harness Studio bir **MCP istemcisi** olduğunda ise başka MCP sunucularının araçlarını Playground'a getirir. Sunucuları **Bağlantılar** içindeki **MCP Sunucuları** alanından ekleyin ya da diğer uygulamalardan içe aktarın. Bu bağlantı ile bir modelin sohbet uç noktasına bağlanmak ayrı şeylerdir.

Yerleşik dosya, komut ve ağ araçlarını diğer MCP istemcilerine sunmak varsayılan olarak kapalıdır. **Ayarlar > Araçlar > Yerleşik Araçlar** içindeki **Araçları MCP istemcilerine sun** seçimiyle açın. İzin politikası geçerliliğini korur; MCP üzerinden size onay soramayan çağrılar reddedilir. Kullanacağınız araçların izinlerini [Araçlar bölümüne](06-araclar.md) göre ayarlayın.

Ayrıca [kodlama ajanı entegrasyonlarına](../../tr/INTEGRATIONS.md) ve [güvenlik modeline](../../tr/SECURITY_MODEL.md) bakın.

Sıradaki bölüm: [Sorun Giderme ve SSS](10-sorun-giderme.md).
