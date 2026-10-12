# Security Policy

[Türkçe](#türkçe)

## Supported versions

Harness Studio is in its first public release series. Only the latest released version receives security fixes.

| Version | Supported |
|---|---|
| Latest 0.x release | Yes |
| Older releases | No; please update |

## Reporting a vulnerability

**Please do not report security problems in public issues, discussions, or pull requests.**

Use one of these private channels:

1. **GitHub private vulnerability reporting** (preferred): open the [Security tab](https://github.com/struklabs/harness-studio/security/advisories/new) of the repository and choose "Report a vulnerability".
2. **Email:** ugur.enes.ustundag@gmail.com. Put "Harness Studio security" in the subject line.

Please include:

- The affected version and your operating system
- A description of the issue and its impact
- Steps to reproduce, or a proof of concept
- Any suggested fix, if you have one

### What to expect

- We acknowledge reports within **3 working days**.
- We give an initial assessment within **7 days** and keep you informed of progress.
- We aim to ship a fix for confirmed high-severity issues within **30 days**, and to coordinate disclosure with you. Lower-severity issues are fixed in a regular release.
- We credit reporters in the release notes unless you prefer to stay anonymous.

This is a small, volunteer-run project; these are targets, not guarantees.

## Scope

Areas that matter most are described in the [security model](docs/en/SECURITY_MODEL.md): secrets and the OS keychain, the downloader and runtime integrity checks, the gateway and Control API (tokens, scopes, network exposure), tool execution and its approval policy, MCP, and writes to third-party client configuration files.

Out of scope: vulnerabilities in third-party software that Harness Studio launches or downloads (llama.cpp, Ollama, and so on; report those upstream), and issues that require an already compromised machine or physical access.

---

## Türkçe

### Desteklenen sürümler

Yalnızca en son yayımlanan sürüm güvenlik düzeltmesi alır.

### Güvenlik açığı bildirme

**Güvenlik sorunlarını herkese açık issue, tartışma veya pull request ile bildirmeyin.** Özel kanallar:

1. **GitHub özel güvenlik bildirimi** (tercih edilen): deponun [Security sekmesinden](https://github.com/struklabs/harness-studio/security/advisories/new) "Report a vulnerability" seçin.
2. **E-posta:** ugur.enes.ustundag@gmail.com (konu: "Harness Studio security").

Bildirimde etkilenen sürümü, işletim sistemini, sorunun açıklamasını ve etkisini, tekrar adımlarını belirtin.

Hedeflerimiz: bildirimi 3 iş günü içinde teyit etmek, 7 gün içinde ilk değerlendirmeyi paylaşmak, doğrulanmış yüksek önem dereceli sorunlar için 30 gün içinde düzeltme yayımlamak. Küçük bir gönüllü proje olduğumuz için bunlar garanti değil, hedeftir. Kapsam için [güvenlik modeline](docs/tr/SECURITY_MODEL.md) bakın.
