# Harness Mobile

A thin Tauri 2 client for iOS and Android built from the desktop's Svelte stack. The phone runs no models. It talks to:

- **a Harness at home or on a server**:
  - the gateway, for chat;
  - the Control API, for jobs, models, modules, devices, performance and approvals;
  - both with one *scoped* named token.
- **bring-your-own-key providers** directly, without a computer: OpenAI, Gemini, Anthropic, OpenRouter and custom OpenAI-compatible endpoints.

Every reply shows where it ran:

- **On a Harness:** the provider, server, device and engine, plus queue and load time.
- **On a provider:** the provider's cloud or a named endpoint (the phone is only the client).
- **Always:** the model's context, tools, vision, tokens and cost when the provider publishes prices.

## Layout

| Path | What |
|---|---|
| `src/` | SvelteKit SPA. Reuses the desktop's theme (`src/app.css`), UI components and translations (`#lib/*` → `../src/lib`; strings under `mobile.*` in `src/lib/locales/mobile.ts`). |
| `src/lib/native.ts` | The only bridge to the native side: HTTP (plain and streamed) and the keychain. |
| `src-tauri/` | Its own Cargo workspace: `tauri`, `reqwest` (rustls), `keyring`. None of the desktop crates. |
| `src-tauri/gen/apple` | The Xcode project (`tauri ios init`); `build/` and `Externals/` are ignored. |
| `src-tauri/gen/android` | The Gradle project (`tauri android init`), with `SecureStorePlugin.kt` (Android Keystore) and the window-inset fix in `MainActivity.kt`. |

## Security model

- **Native HTTP.** Requests leave from Rust (`reqwest`), so they carry no `Origin`; the gateway and Control API need no CORS change. Redirects are not followed.
- **Keys stay in the platform's secret store.**
  - Tokens and keys are stored together with the origin they were entered for: in the iOS Keychain, or on Android encrypted with an AES-GCM key that never leaves the Android Keystore (the ciphertext sits in the app's private preferences).
  - The page names a keychain entry in a request and Rust adds the header. The key is never read back into JavaScript.
  - A key is never sent to another origin.
  - The page can't set `Authorization`, `x-api-key`, `Cookie` or `Host` itself.
- **https off the LAN.** Plain `http` is accepted only for loopback, private and `.local` addresses.
- **Locked-down page.** The CSP allows no network from the page (`connect-src ipc:`), and rendered Markdown is sanitized (DOMPurify; no images or forms).
- **Scoped tokens.** Use a named token (`read,inference`; add `servers:control` to load/unload models, `admin` for approvals and modules), never the session token.

## Develop and build

Install dependencies at the repository root (`npm ci`). Then, from the root:

```bash
npm run mobile:check
```

```bash
npm run mobile:test
```

```bash
npm run mobile:ios -- build --debug --target aarch64-sim
```

```bash
npm run mobile:android -- build --debug --target aarch64 --apk
```

The Android build needs JDK 17, the Android SDK (platform 36, build-tools, NDK) with `ANDROID_HOME` and `NDK_HOME` set, and the `aarch64-linux-android` Rust target. CI checks the Rust side for Android (rustfmt, clippy with the runner's NDK); the APK itself is built locally.

The iOS build needs Xcode, an iOS Simulator runtime, XcodeGen (`brew install xcodegen`) and the Rust targets `aarch64-apple-ios` and `aarch64-apple-ios-sim`. Device and TestFlight builds also need the team's signing settings (`bundle > iOS > developmentTeam`).

## Simulator end-to-end run

Debug builds read one scripted run (`e2e_script` in `src-tauri/src/lib.rs`; release builds ignore it): from the launch environment on iOS, or from `e2e.json` in the app's data folder on Android, which the app deletes once read. The run:

1. pairs with a Harness;
2. streams a chat from its first model;
3. optionally adds a BYOK provider and chats through it;
4. opens Jobs and Manage.

Read the screens with `xcrun simctl io <device> screenshot` or `adb exec-out screencap -p`.

```bash
HARNESS_DATA_DIR=/tmp/hs-e2e harness serve --port 12442 --gateway --gateway-port 12441 --no-restore
```

```bash
HARNESS_DATA_DIR=/tmp/hs-e2e harness api-token create mobile-e2e --scope read,inference,servers:control
```

```bash
SIMCTL_CHILD_HARNESS_MOBILE_E2E='{"gateway":"http://127.0.0.1:12441","api":"http://127.0.0.1:12442","token":"<token>","prompt":"Name three colors."}' xcrun simctl launch booted com.harnessstudio.mobile
```

On the Android emulator, forward the ports to the Mac first. Then copy the script into the app's folder (debug builds are `run-as`-able) and start the app:

```bash
adb reverse tcp:12441 tcp:12441 && adb reverse tcp:12442 tcp:12442
```

```bash
adb push e2e.json /data/local/tmp/ && adb shell "run-as com.harnessstudio.mobile sh -c 'cat /data/local/tmp/e2e.json > e2e.json'" && adb shell rm /data/local/tmp/e2e.json
```

```bash
adb shell am start -n com.harnessstudio.mobile/.MainActivity
```

## Not in this MVP

- **Pairing:** today you type the token into the Connections screen. A pairing code (`feat/remote-pairing-devices`) will implement the same `Pairing` interface (`src/lib/harness.ts`).
- **Server API gaps:** the Control API does not offer a pending tool-call approvals queue, agents, training status or general settings yet. Those sections say so.
- **Background and push:** an answer streams while the app is in the foreground. Background URLSession and push need a native plugin.
