Türkçe: [../tr/PERFORMANCE.md](../tr/PERFORMANCE.md)

# Resource Usage Measurements (Harness Studio, LM Studio, Ollama)

The first section compares idle and loaded RAM/CPU usage of Harness Studio, LM Studio and Ollama side by side and fixes the obvious waste; the idle footprint study follows it, and the later sections cover KV cache precision, concurrent requests and out-of-the-box settings. All numbers were measured on the test systems described in each section; no value was made up or taken from another source.

## Machine and Versions

| | |
|---|---|
| CPU / RAM | AMD Ryzen 7 9700X (8 cores, 16 threads), 31.1 GB |
| GPU | AMD Radeon RX 7900 GRE (16 GB) + integrated Radeon |
| OS | Windows 11 Pro 10.0.26200, display scale 100% (96 DPI), WebView2 154.0.4258.x |
| Harness Studio | 0.1.0, release build (`cargo build -p harness-studio --release --features tauri/custom-protocol`), not an installer package |
| LM Studio | 0.4.25+1 (llama.cpp Vulkan engine 2.47.0) |
| Ollama | 0.34.4 |
| Harness's llama-server | WinGet `ggml.llamacpp` install (the one found by Settings > Runtime) |

## Method

- **Process tree:** everything the application launches is summed: the main process, the WebView2 children (browser, GPU, renderer, helpers) and the inference process (`llama-server`). For LM Studio and Ollama, name matching was added (`LM Studio`, `lms`, `ollama`).
- **Memory:** the Windows performance counters `Working Set - Private` (private working set) and `Private Bytes` (commit), summed over the process tree, sampled every 5 s, averaged over a 60 s window. During generation, every 2 s.
- **CPU:** the `TotalProcessorTime` difference of each process at the start and end of the window. "Single-core %" means 100 = one full logical core; divide by 16 for the percentage of the whole machine.
- **Idle:** 25-85 s after the application started, window open and in the foreground, no model, 60 s.
- **Loaded:** the model loaded and idle for 60 s; then a streaming generation: the same prompt ("a long story about a lighthouse keeper"), with `max_tokens=1500` in Harness, `lms chat` in LM Studio, `ollama run --verbose` in Ollama. Speed (tokens/s) is the value the server reports itself.
- **Same model file:** `gemma-4-12B-it-Q4_K_M.gguf` for LM Studio (the file in LM Studio's own folder; Harness opened the same file in place), context 4096, full GPU. `hermes3:8b` for Ollama (Ollama's own blob; because Harness does not accept a file name with no extension, a hard link to the same file was provided, no copy, deleted at the end of the measurement), context 16384 (chosen by Ollama; `ollama ps`). The measurement with Qwen3-4B-Instruct-2507 Q4_K_M is for Harness only (this model is not in the LM Studio and Ollama stores, and no import was done).
- **Isolation:** The measured Harness ran with a separate data directory and separate ports from any other copy (`HARNESS_DATA_DIR`, `WEBVIEW2_USER_DATA_FOLDER`; see below). LM Studio and Ollama were closed before the measurement; they were opened only for this measurement and then closed. Their settings, models and data were not changed (in LM Studio the model was loaded and unloaded with `lms load/unload`).

## Results

### Idle (no model, window open)

| | Private working set | Commit | CPU (single-core %) | Processes | First window |
|---|---:|---:|---:|---:|---:|
| Harness Studio, before | 200-205 MiB | 289-319 MiB | **22.4** (3 measurements: 22-27) | 7 | 1.2-2.0 s |
| Harness Studio, after | **169-176 MiB** | 285-291 MiB | **2.3** (2 measurements) | 7 | 1.3-1.8 s |
| LM Studio 0.4.25 | 624 MiB (peak 680) | 753 MiB | 0.6 | 6 | 10.6 s |
| Ollama 0.34.4 | 938 MiB (peak 1240), first minute | 1112 MiB | 5.1 | 9 | 2.3 s |
| Ollama, after settling (once a model was loaded and unloaded) | 79 MiB | 341 MiB | 0.07 | 9 | - |

Notes: the first idle values for LM Studio and Ollama are right after startup; Windows later shrinks the working sets (Ollama's main process dropped from 1.1 GiB to 10 MiB, LM Studio's application processes from 624 to about 200 MiB). Harness's working set did not shrink this way (the window stayed open). That is why the real comparison is the application-only total in the "loaded, idle" rows below. "First window" is the moment the main window handle and its title are seen; drawing the content can take additional time. LM Studio's 10.6 s is a single measurement.

### Loaded (model loaded)

Gemma-4-12B-it Q4_K_M, context 4096, full GPU, same file:

| | Loaded, idle: private working set | Commit | Idle CPU | Generation CPU | Speed | Load |
|---|---:|---:|---:|---:|---:|---:|
| Harness Studio (after) | 406 MiB (application 178 + llama-server 227) | 9.3 GiB | 4.2 % | 40.1 % (llama-server 36.3) | 49.9 t/s | 9 s (file in cache) |
| LM Studio | 344 MiB (application 195 + llama-server 149) | 10.0 GiB | 0.4 % | 47.1 % (llama-server 42.7; includes the `lms` client) | 48.0 t/s (1971 tokens, first token 2.9 s) | 61 s (cold disk) |

Hermes3 8B Q4, context 16384, full GPU, same file:

| | Loaded, idle: private working set | Commit | Idle CPU | Generation CPU | Speed | Load |
|---|---:|---:|---:|---:|---:|---:|
| Harness Studio (after) | 354 MiB | 6.8 GiB | 2.5 % | 31.9 % (llama-server 27.7) | 81.0 t/s (1243 tokens) | 7.1 s |
| Ollama | 315 MiB (runner 236) | 4.7 GiB | 0.2 % | 43.5 % (runner 41.9) | 82.8 t/s (1105 tokens) | could not be measured |

Harness only, Qwen3-4B-Instruct-2507 Q4_K_M, context 4096, full GPU (1500 tokens, streaming):

| | Loaded, idle: working set | Idle CPU | Generation CPU | CPU in a minimized window | Speed |
|---|---:|---:|---:|---:|---:|
| Before | 329 MiB | **39.8 %** | **70.3 %** | **50.3 %** | 139.5 t/s |
| After | 311 MiB | **2.75 %** | 38.6 % (llama-server 37) | **0.6 %** | 143.1 t/s |

### Reading the results

- Before the fix, Harness spent a fifth of a core on UI rendering while idle, and half a core when a model was loaded and the window was minimized; LM Studio and Ollama were below 1% while idle (Ollama's startup minute excluded). After the fix the gap shrank (Harness 2.3-4.2 %, LM Studio 0.4-0.6 %, Ollama 0.07-0.2 %); Harness is still slightly higher because it polls state and logs once per second. In a minimized window, polling stops completely (0.6 %).
- In memory, Harness is on the same level as LM Studio on the application side alone (170 MiB idle; while loaded, 178 MiB on the application side versus 195 MiB for LM Studio; LM Studio 624 MiB at startup, Ollama 79 MiB once settled). The WebView2 children are most of Harness's memory share: about 150 MiB.
- Inference speed depends on llama.cpp in all three applications; the measured differences (49.9 vs 48.0 and 81.0 vs 82.8 t/s) are within noise for a single run. The engines are not the same version (LM Studio uses its own Vulkan build, Harness the WinGet build, Ollama its own runner), so the speed and commit comparison includes the engine difference too. Harness's commit value (6.8 GiB versus 4.7 GiB on Hermes3) comes from the memory this build reserves in the GPU driver; details are in the idle footprint section below (q8_0 KV and ROCm on the Ollama side).

## Waste Found and Fixes

The browser side was profiled via CDP (`--remote-debugging-port`): main-thread tasks, layout and style counts, running animations.

| # | Finding | Fix | Measurement |
|---|---|---|---|
| 1 | The VRAM/RAM bar in the telemetry panel used `transition-[width]`; when the value changed every second, layout was recalculated on every frame for 500 ms (1284 layouts in 15 s, main thread 1.94 s) | The bar is drawn with `transform: scaleX`, no transition animation | main thread from 1.94 s to 0.31 s in 15 s (keeping the transition), then to 0.18 s (removing the transition); layouts from 1284 to 12 |
| 2 | The `animate-ping` infinite animation next to every running server in the sidebar; it kept the GPU process running constantly (about 15% of a core by itself while a model was loaded) | Made a static ring | loaded idle CPU from 20.0 % to 2.75 % |
| 3 | WebView2 does not set `document.hidden` to `true` when the window is minimized or hidden to the tray; so the "do not poll while hidden" checks never worked. Also, the 1 s status/telemetry polling on the home page and the Dashboard log polling had no such check at all | The Rust side sends a `window-visible` event when the window is minimized/hidden/restored; the UI (`windowActivity.ts`) combines it with `document.hidden`. The home page, Dashboard logs, Settings > Gateway, Performance and Analytics polling stop while the window is not visible, and refresh immediately when it becomes visible | CPU in a minimized window from 50.3 % to 0.6 % (together with fix 2) |
| 4 | At a high UI scale (see below) the sidebar squeezed the content | When merged with the window frame work, this separate rule was removed; navigation drops to an icon strip below 1024 px (`ResizablePane`). | from the code; not observed in the UI |

Inference speed was not affected by the fixes (139.5, 142.6 and 143.1 t/s); memory did not change significantly either (private working set from 200 to 170 MiB, idle).

Startup time did not change: 1.25-2.03 s before the fix (4 measurements), 1.32-1.85 s after (4 measurements); both are the moment the main window handle appeared and were taken while the machine was under other load.

## High DPI and Small Windows

Not verified by clicking through the GUI; so only a code review was done and the open item is on the ROADMAP.

- The window is at least 980 x 650 (`tauri.conf.json`); the UI scale (90% - 150%) is applied with WebView zoom. At 150%, in a 980 px window the CSS viewport is about 653 x 433 px.
- Fixed widths: sidebar 216 px (56 px when collapsed), the model picker popup 420 px, benchmark table columns 120-240 px (the table scrolls), Hub and Library search boxes at least 220-240 px (the row wraps), the Dashboard switches to a single column at the `max-[860px]` breakpoint, dialogs `max-w-[calc(100%-2rem)]`. Collapsing the sidebar below 720 px was added.
- Not done: scanning in a real window at 125% and 150% Windows scale, a manual check of every screen at 1024 x 640.

## Environment Variable Added for Measurement

`HARNESS_DATA_DIR` (the same variable as the CLI's `--data-dir`) now covers the GUI too: if set, the application uses that data directory and skips the single-instance protection, so an isolated second copy can run next to the normal one. It must be used together with a separate `WEBVIEW2_USER_DATA_FOLDER` and separate port settings (otherwise the WebView2 processes are shared). In normal use the variable is not set and behavior is the same.

## Limits

- Another copy of the application (a debug build) was also open during the measurement; before and after were measured under the same conditions, but the machine had other load. A single 60 s window per condition (two or three repeats for idle); there is no statistical confidence interval.
- Discrete GPU memory (VRAM) was not measured; the commit value includes what the driver reserved.
- LM Studio's load time is a cold disk and Harness's is a warm file cache: they are not comparable. Ollama's load time could not be measured.
- In LM Studio, generation was done with `lms chat` (the server was not opened); the `lms` client (about 190 MiB) also entered the memory and CPU of the generation window, and the memory in the table was taken from the "loaded, idle" row.
- LM Studio's reasoning (thinking) mode was on and it generated 1971 tokens; 1500 in Harness. The speed comparison is approximate.
- Linux and macOS were not measured.

## Idle Footprint: Push Instead of Polling, Closing the Window in the Tray

Goal: to minimize the application's CPU and RAM with no model, with a model loaded but idle, and with the window minimized or in the tray. The idle CPU left over from the first measurement came from the UI's once-per-second status, telemetry and log polling and from the RAM/VRAM sample the backend took every second.

### Method

The method above, with these details:

- **Build:** both sides are `cargo build -p harness-studio --release --features tauri/custom-protocol` (after `npm run build`). "Before" = the build without the change, "after" = the build with it. Both used the same isolated data directory and WebView2 profile in turn.
- **Isolation:** `HARNESS_DATA_DIR` + `WEBVIEW2_USER_DATA_FOLDER` in a temporary folder; settings: Control API 11600, gateway 11635 (both on), server port 8190, llama-server WinGet build (custom binary), close action = tray, guide (tour) off. Another copy of the application with its own data directory and ports was open (with a model loaded) and was not touched.
- **Memory:** the sum of `Working Set - Private` over the process tree, every 5 s, 60 s average. **CPU:** per-process `TotalProcessorTime` difference / 60 s ("single-core %"); the app (harness-studio), WebView2 and llama-server were also summed separately.
- **Scenarios:** 60 s starting 25 s after launch. *Idle*: window open, Dashboard, no model. *Minimized*: `ShowWindow(SW_MINIMIZE)`, measured after 10 s. *Tray*: `WM_CLOSE` to the window (close = tray), measured after 50 s. *Loaded*: Qwen3-4B-Instruct-2507 Q4_K_M (context 4096, full GPU) loaded via the Control API, measured after 20 s. *Loaded + tray*: the same model, window in the tray, measured after 50 s.
- **Startup:** from process start to the moment the main window handle is visible ("first window") and to the moment the `main` element appears in the DOM ("ready to interact"; polled every 25 ms with CDP `--remote-debugging-port`, open only for the startup and restore measurements). Five times each, before and after, alternating.
- **Restore from the tray:** a click on the tray icon was made by sending tray-icon's callback message (`WM_USER_TRAYICON` = 6002, `WM_LBUTTONUP`) to the application's tray window; the time runs from the click until the window is visible and the `main` element exists.
- Each row is a separate launch; the tables show the minimum-maximum range. The raw results and scripts were temporary and were deleted.

### Results

| Scenario | Before: private working set | Before: CPU | After: private working set | After: CPU |
|---|---:|---:|---:|---:|
| Idle, window open, no model (3 before, 10 after) | 171-175 MiB | **1.51-1.75 %** (app 0.16-0.36) | 168-179 MiB | **0.31-1.25 %**, median 0.65 (app 0-0.08) |
| Minimized, no model | 165-166 MiB | 0.26-0.44 % | **78-80 MiB** | 0.29-0.36 % |
| In the tray, no model | 162 MiB (7 processes) | 0.34-0.52 % | **15-16 MiB (1 process)** | **0.00 %** |
| In the tray, "Keep the window loaded in the tray" on | - | - | 73 MiB | 0.08 % |
| Model loaded, idle, window open | 314-316 MiB (llama-server 141) | **2.50-2.63 %** (llama-server 0.49-0.52) | 320-331 MiB (llama-server 141) | **0.52-1.61 %**, median 0.78 (llama-server 0.18-0.52) |
| Model loaded, window in the tray | 312 MiB | 0.65-0.75 % | **59-158 MiB** (app 16 + llama-server 42-141) | **0.16-0.31 %** |

In the first measurement round the guide (tour) window was open; in that round "before" idle was 1.85-3.44 % (the same level as the 2.3 % in the first measurement). The "before" rows above are from the second round with the guide off.

Startup (5 + 5 measurements, alternating):

| | First window | Ready to interact | Memory (5 s after startup) |
|---|---:|---:|---:|
| Before | 718-795 ms (median 721) | 941-1020 ms (median 958) | 191-194 MiB |
| After | 692-771 ms (median 736) | 896-980 ms (median 934) | 196-199 MiB |

The difference is within noise (~25 ms). The initially loaded JavaScript went from 2.06 MB to 1.25 MB (tabs load when first opened), but on a warm WebView2 profile this did not measurably shorten startup: on this machine most of startup is WebView2 itself.

Restore from the tray:

| | Window visible | Ready to interact |
|---|---:|---:|
| Before (the window was waiting hidden) | 11-16 ms | 40-104 ms |
| After, default (the page had been closed and was rebuilt) | 312-491 ms | 516-741 ms |
| After, "Keep the window loaded in the tray" on | 16 ms | 22 ms |

Per process (idle, after; private working set / CPU): harness-studio 16 MiB / 0-0.08 %, WebView2 browser 33-35 MiB / 0.08-0.18 %, GPU process 48-60 MiB / 0-0.08 %, renderer 47-56 MiB / 0.03-0.6 %, network service 10 MiB, storage service 3 MiB, crashpad 2 MiB. The renderer's main thread runs 23 ms in 20 s while idle (CDP `Performance.getMetrics`: 4 layouts, 4 style calculations); the remaining CPU is Chromium's own work for a visible window.

llama-server itself (outside the application, same model, 60 s): 0.13 % when not polled, 0.29 % when `/metrics` + `/slots` are polled once per second. Half of the llama-server share in the loaded-idle row comes from polling; with the window open, polling once per second continues (below).

### What Changed

| # | Finding | Change |
|---|---|---|
| 1 | The home page asked for `list_servers` + `get_telemetry_snapshot` every second, the gateway status every 3 s, the Dashboard logs every second, and the download queue every 3 s (even when minimized) | The backend sends an event when something changes: `servers-changed` (with the new list), `server-logs` (servers with new lines, merged within 250 ms), `downloads-changed`, `gateway-changed`. Server state, errors, restarts and registration/deletion are reported through `harness_core::changes` (a no-load, coalescing `watch` channel; log lines do not enter the Control API's event stream). There are no polling timers left in the UI; it reads once when the window becomes visible again. |
| 2 | The backend sampled RAM + VRAM every second, even with no server and nobody looking; the PDH query for VRAM was opened and closed every time | The sampler sleeps completely when no server is running and no visible view needs it. It samples every 2 s while the Dashboard's memory indicators are visible, every second while a server is running with the window open, and every 5 s when the window is closed/small and servers are idle; it goes back to once per second while a request is being processed, and takes a sample immediately when a gateway request ends (for the memory peak in analytics). The PDH query is kept open (`gpu::VramMonitor`), the sysinfo object is reused and only memory is refreshed. Changed telemetry is pushed with the `telemetry` event, only for the watched server and only when there is a meaningful difference (server fields; 64 MiB for RAM/VRAM). When the instantaneous value of a stopped server or the Control API is requested, memory is read at that moment (`TelemetryPoller::fresh_snapshot`). |
| 3 | The gateway maintenance loop read model definitions from disk every second (a 1 s cache) | If the gateway has no loaded model, it reads nothing. |
| 4 | A minimized/in-tray window kept its WebView2 memory as it was | When the window becomes invisible, `ICoreWebView2_19::MemoryUsageTargetLevel = Low`, and `Normal` when it becomes visible (`webview2-com`). While minimized, the GPU process went from 50 to 3 MiB and the renderer from 50 to 6 MiB. |
| 5 | A window in the tray held 7 processes and ~160 MiB | 30 s after the window is hidden to the tray, if the page has no request in flight, has been quiet for 10 s, and no server is processing a request, the web view is closed (`release_window`); the tray icon/menu or a second launch rebuilds the window at the same position and size, and the open tab comes back. Servers, downloads, the gateway and the Control API are in the backend, so they are unaffected. Settings > Appearance & Startup > "Keep the window loaded in the tray" turns this off (off by default). |
| 6 | At startup the first screen waited for the hardware scan and the offline crash report check; the GPU list (DXGI) was fetched before the window was created; all tabs were in a single 2 MB JS chunk | The first screen waits only for settings, servers and profiles; it fills in when the hardware arrives, and the crash check comes later. The GPU list is fetched in the background after the window opens. Tabs other than the Dashboard, the setup wizard and the preset manager load when first opened (`LazyView`). |
| 7 | The telemetry sampler could read the settings and cache them for the tray tooltip before the data directory (`paths::init`) was set; in that case the application opened with default settings for that whole session (seen once in 15 launches in measurement) | The sampler and push tasks start after `paths::init`. |

Evaluated and not adopted (idle, window open, after build, two measurements each):

| WebView2 argument | Private working set | CPU | Decision |
|---|---:|---:|---|
| Default (Tauri's `--disable-features=msWebOOUI,msPdfOOUI,msSmartScreenProtection`) | 171-174 MiB | 0.31-0.39 % | |
| + `--disable-gpu` | 127-131 MiB | 0.44-0.47 % | Not adopted: the GPU process goes 48 -> 9 MiB, but drawing is done in software (CPU); CPU during use (scrolling, a streaming response) was not measured. |
| + `--enable-low-end-device-mode` | 158-165 MiB | 0.36-0.68 % | Not adopted: ~10 MiB gain; textures with low color depth can degrade the image. |

### Reading the results

- With no model and the window open, idle CPU dropped from 1.5-1.75 % to a median of 0.65 % (0.31-1.25); the application process is nearly zero (0-0.08 %), and the rest is what WebView2 spends on a visible page. LM Studio was 0.4-0.6 % (first measurement). The 0.5 % target was met in three of ten measurements; the value varies with other load on the machine, and the remaining share is Chromium's, not the application's.
- Model loaded, window open: from 2.5-2.6 % to a median of 0.8 %. Of this, 0.2-0.5 is llama-server; since telemetry looks live while the window is open, once-per-second polling continues.
- In the tray with no model, Harness is now just its own process: 15-16 MiB and 0 % CPU (Ollama 79 MiB, 0.07 % once settled). In the tray with a model loaded, 0.16-0.31 % (llama-server's own idle share is 0.13 %).
- The cost: restoring from the tray takes 0.5-0.75 s (about 0.1 s before). Unsaved input (unsent text in the Playground, unsaved form changes on the Dashboard) is lost when the page closes; the open tab comes back. It can be turned off with the setting.

### Limits

- A development copy (with a model loaded) and other work were open on the machine; the CPU rows are ranges of a few measurements, with no confidence interval. Whether another window was on top of the window was not checked (Chromium may not draw a covered window); before and after were measured under the same conditions.
- Startup is with a warm WebView2 profile; cold startup (first install, after a restart) was not measured.
- The restore time was measured by sending the tray message, not by a real click on the icon. The window opening at the same position and size, maximized, and the tab coming back are from the code; they were not verified on screen.
- `--disable-gpu` was measured only while idle.
- Windows only. `MemoryUsageTargetLevel` does not exist on Linux and macOS (the call is not made); closing the page in the tray works with the same code but was not tried there.

### macOS

MacBook Air M1 8 GB, macOS 27.0.1, release build (`/Applications/Harness Studio.app`, ad-hoc signed). Memory: the sum of `phys_footprint` of the application, the WebKit processes (WebContent, GPU, Networking) and the llama-server started after the application; CPU: the `ps` CPU time difference of the same process set / 60 s.

| Scenario | Memory | CPU |
|---|---:|---:|
| Window open, no model | 174 MiB | 1.28 % |
| Minimized (Dock), no model | 171 MiB | 0.12 % |
| In the tray, no model (page released) | **43 MiB** | **0.00 %** |
| Model loaded (Qwen3-4B, 4k), window open | 867 MiB | 3.05 % |
| Model loaded, in the tray (from the Library page, page released) | application 44 + llama-server 665 MiB | 0.47 + 0.68 % |

- Releasing the page in the tray works on macOS too (WebContent closes within ~1 min).
- **Known issue:** if the window is sent to the tray while the **Dashboard** page is open and a server is running, the page is not released (after 5 min WebContent is still open, 914 MiB in total); in the same situation it is released from the Library page. The cause has not been found.
- **Known issue:** on a hidden start with `--minimized` + "Start minimized", the page is never released (after 90 s WebContent is 127 MiB): because the `window-in-tray` event does not arrive at startup, the release timer is not set.
- Minimizing (Dock) does not lower memory, only CPU.
- llama-server's 665 MiB does not count the `mmap`ped part of the weights; for real memory see the Metal memory table below.

### Per-Process VRAM and Multi-Server Telemetry

To split the Dashboard's VRAM bar by consumer, the GPU memory dedicated per process is also read on every telemetry round (Windows: PDH `\GPU Process Memory(*)\Dedicated Usage`). There is no new timer: the counter is read from a second PDH query kept open, in the same round that reads system VRAM; it sleeps along with the sampler, which sleeps when the round is idle.

**Method.** The test machine (Ryzen 7 9700X, RX 7900 GRE + integrated Radeon, Windows 11). On a release build, after `VramMonitor` was opened and warmed up, 25 rounds, 1 s apart (the real round interval), with `usage()` (existing) and `process_usage()` (new) timed separately on every round; median and maximum. At the same time, 37 processes held GPU memory (a development app + a llama-server and browsers). A temporary test file was used for the measurement and deleted.

| Reading (per round) | Median | Maximum |
|---|---|---|
| System VRAM (`usage()`, existing) | 43 µs | 93 µs |
| Per-process VRAM (`process_usage()`, new) | 97 µs | 140 µs |
| First call (including opening the query, once) | 2.1 ms | |

**Reading.** The added cost per round is ~0.1 ms (at 1 Hz about 0.01 % of a core per second; below measurement noise), and the memory is a `HashMap` of 37 samples. The process table was already refreshed on every round for the RAM breakdown; tree building is done from the same table, with no extra reads. Watching all servers adds no extra sampling (the sampler already reads them all); only the `telemetry` event is pushed separately for each changed server, and only while the Dashboard telemetry panel is open. The effect on idle CPU was not measured separately: this work is done only while the sampler is running (while a server exists or a memory indicator is visible).

**Limits.**
- The Linux (`/proc/<pid>/fdinfo`) path has unit tests but was not measured on the test machine; its cost grows with the number of open files of the processes in the tree (only files that have a DRM line are parsed).
- On macOS and on Linux with NVIDIA the per-process counter is not used (`nvidia-smi` would start a process every round), and the bar is not split.
- Only the monitored (primary) graphics card; on Windows only `Dedicated Usage`.

### Restore from the Tray: Pre-Building and the Launch Screen

Opening a window whose page had been closed from the tray was visible in 0.3-0.5 s and usable in 0.5-0.75 s (above). The change:

- When the cursor reaches the tray icon (`TrayIconEvent::Enter`/`Move`; Windows and macOS) or the icon is pressed (`Click` + `Down`, including right-click), the window is built hidden in the background (`prewarm_main_window`); a click only shows it. If it is not shown within 20 s, it is closed again the same way as `release_window`; the time is extended while the cursor stays on the icon. The Linux AppIndicator tray gives no icon event.
- Rebuilding while the WebView2 browser process of the closed page was still shutting down (same profile) did not load the new page; now the exit of that process is awaited first (at most 5 s on a click; on hover it is deferred to the next event without waiting).
- A launch screen in `src/app.html` that waits for no script; the window and web view are built with the theme's background color.

**Method:** the method above: `cargo build -p harness-studio --release --features tauri/custom-protocol`, "before" = the build without the change, "after" = the build with it; the same isolated data directory and WebView2 profile (`HARNESS_DATA_DIR`, `WEBVIEW2_USER_DATA_FOLDER`), Control API 11600, gateway 11635, server port 8190, close action = tray, CDP 9351. Five + three rounds before and after, each round a separate launch, alternating. Before every measurement, `WM_CLOSE` was sent to the window (to the tray), the page was closed (the window disappeared) and 4 s were waited. Tray events were sent to tray-icon's window with the `WM_USER_TRAYICON` (6002) message: hover is `WM_MOUSEMOVE`, a click is `WM_LBUTTONDOWN` + 90 ms + `WM_LBUTTONUP`. Times are from `WM_LBUTTONUP`: **visible** = the window handle is visible (on a separate thread every 2 ms), **ready** = with CDP, the `main` element exists and the launch screen is absent or going away (in the before build there is no launch screen, only `main`). Other builds and a llama-bench were running on the machine.

| Scenario (8 repeats) | Before: visible | Before: ready | After: visible | After: ready |
|---|---:|---:|---:|---:|
| Hover, click 1 s later | 327-650 ms (median 385) | 579-973 ms (676) | **15-20 ms (16)** | **3-6 ms (4)** |
| Hover, click 0.3 s later | - | - | **16-48 ms (16)** | **151-352 ms (258)** |
| Click without hovering (press -> 90 ms -> release) | 341-463 ms (381) | 591-731 ms (644) | 250-419 ms (305) | 488-669 ms (558) |
| Release message only (like "Show" from the menu) | 339-428 ms (363) | 592-704 ms (634) | 352-500 ms (406) | 577-772 ms (662) |
| Startup (from process start, 6 repeats) | 813-1022 ms (964) | 1086-1284 ms (1196) | 771-1127 ms (964) | 1035-1414 ms (1177) |

Hover without a click: with the page closed, 1 process / 16 MiB; 3 s after hovering, 15 processes / 179-192 MiB (window hidden, page loaded); after 25 s, again 1 process / 17-18 MiB (3 measurements).

Reading:
- Moving the mouse to the icon and clicking (hovering always happens) now shows an already-loaded page: visible in ~16 ms, page ready. If the cursor stays on it for 0.3 s, the page is ready ~0.25 s after the click. Without a hover event, the gain is the time between press and release (~75 ms).
- The release-message-only path (the menu route) is within noise (median +43 ms visible, +28 ms ready); "after" does not rebuild before the old browser process has exited.
- Every hover without a click costs ~175 MiB for 20 s; afterwards the footprint in the tray is the same (16-18 MiB).

Launch screen and font:
- `first-contentful-paint` (WebView2) was reported ~2.2 s after the click in both builds (`DOMContentLoaded` ~0.35 s); while the desktop was shared with other windows (Chromium does not draw a covered window), the first paint time could not be measured reliably. When the launch screen and the pre-built page appear on screen should be verified visually (see the [ROADMAP](ROADMAP.md)).
- From a screenshot: a window opened from the tray shows the theme's background color (`#0f1115` in the dark theme), not white, until the page is drawn.
- Size: the UI bundle 7,609,309 -> 7,689,637 B (+80 KB; IBM Plex Sans Latin 45.7 KB + Latin Extended 31.0 KB, variable weight; CSS +0.9 KB). `harness-studio.exe` 24,839,168 -> 24,927,232 B (+86 KB, +0.35 %). Four static weights (400/500/600/700) in the same two subsets would be 159 KB; the variable file was chosen. The Latin Extended file loads only when those characters appear.

Limits: tray events were produced by sending messages, not tried with a real mouse; hover on macOS exists in the code but was not tried; a single machine, under load, with no confidence interval.

---

# Inference Memory, llama-server Defaults, Size and Disk

Goal: to find where Harness's commit value for the same model file comes from (6.8 GiB on Hermes3 8B, against 4.7 GiB for Ollama), to shrink the llama-server defaults that grow RAM, and to measure the idle cost of `harness serve`, the binary and installer sizes, and the growth of the data directory. All numbers were measured on the test machine (machine and versions are the same as in the table above).

## Method

- **llama-server directly:** a copy of the managed runtime (b11302 Vulkan) in a scratch directory; `-ngl 999 --split-mode none --main-gpu 0 -c 16384` and the measured flags. Once `/health` was ready, it waited 8 s and measured; then 3 generations of 400 tokens (the same prompt, `seed`) and measured again. Memory: `Win32_PerfFormattedData_PerfProc_Process` (`WorkingSetPrivate` = private working set, `PrivateBytes` = commit); VRAM: `GPU Process Memory(pid_*)\Dedicated Usage`. Speed and prompt time are the server's own `timings` values.
- **Growth:** to the same server, (a) 6-8 separate requests made of different ~2000-word slices of `docs/ARCHITECTURE.md` (~4k tokens), (b) a single 20-turn conversation with ~400 words added each turn. Commit was sampled after every request.
- **Ollama:** 0.34.4, `ollama serve` on a separate port (`OLLAMA_HOST=127.0.0.1:11977`) only for the measurement; the `hermes3:8b` blob, `num_ctx=16384`; the runner's command line was read from the process. Shut down at the end of the measurement.
- **Harness itself:** `harness serve --data-dir <temporary> --port 18600 --gateway --gateway-port 18601 --no-restore`, a 60 s window 20 s after startup (a sample every 5 s), before/after alternating 3 times each.
- **Size:** `cargo build --release` (workspace profile: `lto = true`, `codegen-units = 1`, `panic = "abort"`, `strip = true`), the GUI with `--features tauri/custom-protocol`; installer packages with `npx tauri bundle -b nsis,msi` (unsigned). For the speed comparison, a separate measurement program calling `harness-core` functions was built with the same profile (not committed).

## Commit Difference: Harness 6.6 GiB, Ollama 4.5 GiB (Hermes3 8B Q4, context 16384, full GPU)

| | VRAM | Commit | Private working set (idle / after 3 generations) | Generation |
|---|---:|---:|---:|---:|
| Harness default (`-np` auto = 4, `-fa` auto, KV f16) | 6456 MiB | 6583-6618 MiB | 139-166 / 269 MiB | 97.5-99.9 t/s |
| `--no-host` | 6456 | 6582 | 139 / 252 | 100.7 |
| `GGML_VK_DISABLE_HOST_VISIBLE_VIDMEM=1` | 6459 | 6997 | **550 / 663** | 96.2 |
| `-np 1 -fa on -ctk q8_0 -ctv q8_0` | **5495** | **5655** | 159 / 219 | 96.5 |
| Ollama's flags, on Harness's Vulkan build (`+ -b 1024 -ub 1024`) | 5611 | 5769 | 170 / 232 | 95.7 |
| **Ollama 0.34.4** (runner = its own llama-server, **ROCm**) | 5809 | **4534** (4618 after generation) | 243 / 308 | 84.8-86.2 |

`-np 1` and `-fa on` alone did not change VRAM or commit (6456 / 6581-6583 MiB; speeds are not listed because they were measured with a single generation).

The difference comes from three things, two of which are not in Harness's hands:

1. **KV cache type.** Ollama starts its runner with `--cache-type-k q8_0 --cache-type-v q8_0 --flash-attn on -np 1 -b 1024 -ub 1024 --context-shift --keep 4`. The q8_0 comes from user environment variables on the test machine (`OLLAMA_KV_CACHE_TYPE=q8_0`, `OLLAMA_FLASH_ATTENTION=true`); Ollama's own default is f16. The KV goes from 2048 MiB to 1088 MiB. With the same setting Harness also drops to 5.5 GiB VRAM / 5.7 GiB commit.
2. **Driver accounting.** AMD's Vulkan driver writes device memory allocations to the process's commit almost one to one (commit ≈ VRAM + ~150 MiB). Ollama chooses the ROCm backend on this machine; there, 5.8 GiB of VRAM shows up as 4.5 GiB of commit. This commit is not physical RAM, but an allocation deducted from the system's commit limit. In physical RAM (private working set), Harness's llama-server is smaller than Ollama's runner: 139-166 against 243-308 MiB.
3. **Loaded idle 354 versus 315 MiB** (earlier measurement): since Harness's llama-server is smaller, the difference is on the application side (WebView2). That part is covered by the idle footprint work above.

**The KV default stayed f16:** with q8_0, prompt processing is 10-25% slower on this Vulkan build (table below: 1609-1918 against 2080-2147 t/s), and generation speed is the same. "Prepare Server" (agent tools) already chooses q8_0; it can be chosen on the Dashboard.

**`-np` auto (4 slots, unified KV) was left unchanged at this stage:** for models without a sliding window it costs nothing (VRAM is the same). For models with a sliding window (Gemma), the SWA cache grows with the slot count: on Gemma-4-12B, `-np 1` lowers idle commit from 9287 to 8316 MiB (~1 GiB of VRAM) and shrinks every context checkpoint. The cost: requests that arrive at the same time queue (agents, sub-agents, several tools connected to the gateway). It is a trade-off; the VRAM estimate (`vram.rs`) already counts the automatic slot as 4 unified slots. The current Auto rule is described in the concurrent requests section below.

## Real RAM Growth: Prompt Cache and Context Checkpoints

Two llama-server defaults grow physical RAM by GiBs as the server is used:

- `--cache-ram 8192`: the state of finished conversations is kept in system memory (up to 8 GiB).
- `--ctx-checkpoints 32` (per slot): copies of the conversation on models with a sliding window and hybrid models (Gemma, gpt-oss, Qwen3.5); about two per request. On Gemma-4-12B (4 slots) one is 320 MiB. `--cache-ram` does not limit these.

Hermes3 8B, 8 different ~4k-token requests (KV f16):

| | Private working set: idle -> last | Commit: idle -> last | Prompt processing |
|---|---:|---:|---:|
| llama-server default | 152 -> **4741 MiB** | 6595 -> 11259 MiB | 2106 t/s |
| `-np 1` | 150 -> 4731 | 6594 -> 11243 | 2084 |
| **`--cache-ram 1024`** | 151 -> **835** | 6594 -> 7325 | 2147 |
| `--cache-ram 0` | 150 -> 164 | 6593 -> 6653 | 2080 |
| q8_0 KV, `-fa on`, `--cache-ram 1024` | 152 -> 871 | 5634 -> 6443 | 1918 |
| q8_0 KV, `-fa on`, `--cache-ram 0` | 152 -> 172 | 5634 -> 5766 | 1609 |
| f16 KV, `-fa on`, `--cache-ram 0` | 151 -> 170 | 6595 -> 6659 | 2124 |

Gemma-4-12B Q4 (sliding window 1024), context 32768, a single 20-turn conversation:

| | Private working set: idle -> last | Commit: idle -> last | Prompt processing |
|---|---:|---:|---:|
| llama-server default | 217 -> **7582 MiB** | 9577 -> 19851 MiB | 293 t/s |
| `--cache-ram 1024` | 217 -> 8728 | 9577 -> 19820 | 320 |
| **`--cache-ram 1024 --ctx-checkpoints 4`** | 217 -> **1438** | 9577 -> 11480 | 320 |

Gemma-4-12B, context 16384, 6 different requests: default 230 -> 6083 MiB, `--cache-ram 1024` 200 -> 805 MiB, `-np 1 --cache-ram 1024` 194 -> 763 MiB.

**New defaults (if the user has not chosen a value):** `--cache-ram 1024` and `--ctx-checkpoints 4`. They are sent only if the runtime recognizes the flag (an unknown flag is not sent to an old build, and no warning is written). If there is a catalog value or any spelling of the flag in the custom arguments (`-cram`, `--swa-checkpoints=8` ...), Harness does not add its own value; the user can go back to llama-server's values (`8192`, `32`) or to `-1`. A `ctx_checkpoints` parameter was added to the catalog (Advanced > Memory); the tooltips of the two fields show Harness's value. Verified in the command line of a server loaded from the CLI (`… --metrics --split-mode none --main-gpu 0 --cache-ram 1024 --ctx-checkpoints 4`).

Their costs (Gemma-4-12B, 8 turns, ~8k tokens, `--cache-ram 1024`):

| | 4 checkpoints | 32 checkpoints |
|---|---:|---:|
| Regenerate the last response | 1 token, 97 ms | 1 token, 100 ms |
| Edit the last user message | 12 tokens, 189 ms | 12 tokens, 259 ms |
| Edit the message 4 turns back | **3655 tokens, 4.6 s** | 13 tokens, 0.5 s |

- On a model with a sliding window, going back a few turns and editing a message makes the prompt be processed from the beginning (Gemma-4-12B ~800 t/s on this GPU). Regenerating and editing the last message are not affected.
- When switching back and forth between more than 1 GiB worth of conversations, the prompt of the conversation returned to is reprocessed (~4k tokens on Hermes3 ≈ 2 s).
- The memory estimate (`vram.rs`) did not change: VRAM is the same. The prompt cache and checkpoints are system memory that grows while running; they are not part of the load estimate, and are now capped by these two limits (about 1 GiB on Gemma-4-12B + 4 × 320 MiB per active slot).

## `harness serve` Idle

| | Private working set | Commit | CPU (single-core %) | Threads | Data directory (80 s) |
|---|---:|---:|---:|---:|---:|
| Before (3 measurements) | 9.8-10.2 MiB | 11.3-11.9 MiB | 0.05-0.15 | 20 | 36 KiB |
| After (3 measurements) | 9.8-10.0 MiB | 11.4-11.6 MiB | 0.05-0.17 | 20-21 | 36 KiB |

Headless mode is already small; the difference is within noise. What was done: the gateway scheduler's per-second TTL round no longer reads the definitions when the gateway has not loaded a model (before, a `state.db` query every second). sysinfo's `multithread` feature was turned off (no rayon pool; refreshing the process list does not start extra threads). The 1 Hz telemetry sampler is covered above.

## Size

| | Before | After | Difference |
|---|---:|---:|---:|
| `harness-studio.exe` | 29,875,712 B (28.5 MiB) | 24,898,048 B (23.7 MiB) | **-16.7 %** |
| `harness.exe` | 18,441,216 B (17.6 MiB) | 15,131,136 B (14.4 MiB) | **-17.9 %** |
| `harness-hidden.exe` | 214,016 B | 203,264 B | -5.0 % |
| NSIS installer | 15.40 MiB | 13.58 MiB | **-11.8 %** |
| MSI | 22.76 MiB | 19.36 MiB | **-14.9 %** |
| UI bundle (`build/`) | 7.55 MB, 128 files | unchanged | |

What was done:

1. **Dependencies** (`harness.exe` 18.44 -> 18.19 MB): `zip` only Deflate/Deflate64 (zstd, bzip2, xz, AES gone; llama.cpp archives and DOCX are Deflate), `sysinfo` 0.33 -> 0.38 and only `system` + `disk` (the rayon, network, component and user modules gone), `windows` 0.61 -> 0.62 (a single `windows` version left in the CLI, previously 0.57 + 0.61), `toml` 0.8 -> 1 (the same as Tauri; toml 0.8, toml_edit 0.20 and their companions gone). The remaining duplicate versions come from Tauri, the updater and OTel (`reqwest` 0.12 + 0.13, `png`, `infer`, `rand`, `getrandom`, `windows` 0.61 from `tauri-plugin-opener`).
2. **Per-package `opt-level = "s"`** (`harness.exe` 18.19 -> 15.13 MB): code that is not on the hot path: the HTTP routing layer (axum, tower), CLI parsing (clap), OpenAPI (utoipa), OTLP (opentelemetry*, tonic, prost, rustls), `harness-api`/`-mcp`/`-cli`/`-gateway`/`harness-studio` and the Tauri/WebView shell. `harness-core` (hash, GGUF, RAG), sha2, the PDF/zip decoders, serde_json, SQLite, tokio and hyper stayed at 3. It was measured that the per-package setting also holds under full LTO (binary `s`, `harness-core` + `sha2` 3: SHA-256 1079 ms, GGUF 3.3 ms, chunking 1.9 ms).

Making everything `s` brings `harness.exe` down to 11.85 MB but lowers speed; so it was not done:

| (measurement program, 2 repeats) | `opt-level = 3` | everything `s` |
|---|---:|---:|
| SHA-256, 2.4 GB model file (download verification) | 1212-1227 ms | **4777-4800 ms** |
| PDF text (66 KB) | 4.7-4.9 ms | 7.4-7.5 ms |
| Chunking (4 × ARCHITECTURE.md) | 1.7-1.8 ms | 2.4-2.5 ms |
| GGUF metadata | 3.5 ms | 4.6 ms |
| Nearest 8 (50k × 768 vectors) | 19.1-19.6 ms | 21.0-21.4 ms |
| JSON read / write (41 MB) | 41.6-43.0 / 44.6-46.1 ms | 44.9-46.4 / 39.7-39.9 ms |

`panic = "abort"` was already on; the RAG PDF child process (`src-tauri/src/main.rs`) is written for it and did not change. `lto = true` (fat) and `codegen-units = 1` were also already on.

**The size cost of OpenTelemetry** (`harness.exe`, measured before steps 1 and 2 above):

| `otel` | `harness.exe` | Difference |
|---|---:|---:|
| Off | 15,975,424 B | - |
| HTTP/protobuf only, via reqwest 0.12 (a ~20-line `HttpClient` adaptation; a trial, not committed) | 16,595,456 B | +0.62 MB |
| HTTP/protobuf only, reqwest 0.13 (`reqwest-client`) | 16,935,424 B | +0.96 MB |
| As it was: HTTP + gRPC (tonic, rustls/ring, root certificates) + reqwest 0.13 | 18,439,680 B | **+2.46 MB (+15.4 %)** |

gRPC alone is 1.50 MB, the second reqwest 0.34 MB, the OTel SDK + HTTP exporter 0.62 MB. Options: move gRPC into a separate feature (`otel-grpc`, not in the default build), send OTLP/HTTP via reqwest 0.12 (the second reqwest goes away), or take `otel` out of the default build. The first two were done (below).

### OpenTelemetry Slimming

OTLP/HTTP goes through the workspace's reqwest 0.12 (`otel/export.rs`, an `opentelemetry_http::HttpClient` adaptation); the `reqwest013` dependency is gone. gRPC (tonic, rustls/ring, root certificates) moved to the `otel-grpc` feature; it is not in the default builds. The same machine, the same profile (including the `opt-level = "s"` settings above), the same UI bundle; `cargo build --release -p harness-cli` and `-p harness-studio --features tauri/custom-protocol` separately:

| | Before (`otel`: HTTP + gRPC + reqwest 0.13) | After (`otel`: HTTP only, reqwest 0.12) | Difference |
|---|---:|---:|---:|
| `harness.exe` | 15,219,200 B | 14,641,152 B | **-578,048 B (-3.8 %)** |
| `harness-studio.exe` | 25,006,080 B | 24,652,800 B | **-353,280 B (-1.4 %)** |
| `harness.exe --features otel-grpc` | | 14,913,024 B | gRPC = +271,872 B |

After `opt-level = "s"`, gRPC's share is 0.27 MB, not 1.50 MB; the second reqwest (and its hyper-tls/native-tls copy) is 0.31 MB. In the GUI, reqwest 0.13 stays because it comes from `tauri-plugin-updater`; the GUI's gain is gRPC + OTel's reqwest 0.13 features. The default `harness.exe` no longer has `tonic`, `rustls` and `reqwest` 0.13 (`cargo tree -i`, no `grpc-status`/`rustls` strings in the binary).

## Disk: Data Directory Growth

| What | Limit |
|---|---|
| Server logs `logs/servers/<id>/` | Before: the last 20 runs per server, but **a single run's file was unbounded** (a heavily used server left open for weeks could write GBs). After: when a file exceeds 16 MiB, the same run continues in a new file; per server, the newest files are kept up to 20 files and 64 MiB in total (the newest file always stays). A file opened in the same millisecond does not overwrite an existing one. |
| `logs/diagnostics` | 50 files (unchanged) |
| `analytics.db` | Raw requests 30 days, daily rollups permanent (small), content 7 days; maintenance hourly, ending with `wal_checkpoint(TRUNCATE)` (unchanged) |
| `bin/staging` | Deleted after installation (unchanged) |
| `bin/backup` | The previous single runtime (for rollback; 91 MB on the test machine). A single copy, replaced with the new one on every update |
| `.part` files of model downloads | They stay so downloads can resume; they go when the download is removed from the list with its partial data (by design) |

## Limits

- A single machine, AMD Vulkan driver. The commit ≈ VRAM relationship may be specific to this driver; NVIDIA/CUDA, Linux and macOS were not measured.
- Speed measurements are the average of 3 generations (first table) or a single series (growth tables); there is no statistical confidence interval. The prompt-processing speed difference (f16 / q8_0) is consistent across two series.
- Because Ollama uses ROCm and Harness uses Vulkan, the speed and commit comparison includes the backend difference too.
- The UI's (WebView2) idle memory, the telemetry sampler and the window/tray code are outside this study (covered above).
- The installers were produced unsigned and without updater artifacts; WebView2 is not in the package (the default download bootstrapper).

## Low-End Hardware (Debian, 4 GB RAM, Intel HD 6000)

Debian 13 XFCE/X11, Intel Core i5-5250U (2 cores / 4 threads), 3.7 GiB usable RAM + 5.8 GiB swap, Intel HD Graphics 6000 (Broadwell GT3, Mesa Vulkan, `uma: 1`, 1.9 GiB visible in Vulkan0), managed runtime b11316 (Vulkan). Measurements were made with the UI closed.

### Method
- `llama-bench -t 2 -p 512 -n 128 -r 3`, each run bounded with `timeout 600` (because of the hang below). 4 threads were slower than 2 in generation (hyper-threading), so the series is with 2.
- Three placements: **CPU** = `-dev none -ngl 0` (no Vulkan), **ngl 0** = Vulkan loaded, weights on the processor (part of prompt processing on the GPU), **ngl 99** = everything on the integrated GPU.
- Peak RSS is `/usr/bin/time %M`. In the Vulkan placements the weights sit in shared memory mapped to the GPU, so they do not show up in RSS (they still come out of system RAM); in the CPU placement the weights are repacked, so RSS is larger than the file.
- All models are Q4_K_M, context is the llama-bench default (pp512 / tg128).

### Results (t/s, prompt processing pp512 / generation tg128)

| Model | File | CPU | ngl 0 | ngl 99 | CPU peak RSS |
|---|---|---|---|---|---|
| Qwen2.5-0.5B-Instruct | 469 MB | 55 / 21.1 | 130 / **28.2** | 146 / 20.3 | 657 MB |
| Qwen3-0.6B | 378 MB | 93 / **30.1** | 91 / 26.0 | 96 / 16.0 | 783 MB |
| Llama-3.2-1B-Instruct | 770 MB | 44 / **15.9** | 52 / 13.7 | 54 / 9.9 | 1419 MB |
| gemma-3-1b-it | 769 MB | 21 / 8.7 | 72 / 7.5 | 77 / **9.8** | 1021 MB |
| phi-1_5 | 852 MB | 21 / 10.0 | 42 / 9.4 | 43 / 10.0 | 1592 MB |
| Qwen2.5-Coder-1.5B-Instruct | 1066 MB | 23 / **9.8** | 42 / 8.6 | 43 / 8.8 | 1861 MB |
| SmolLM2-1.7B-Instruct | 1007 MB | 19 / **9.0** | 32 / 8.0 | 33 / 7.8 | 1990 MB |
| Qwen3-1.7B | 1056 MB | 21 / **8.3** | 36 / 7.3 | 38 / 6.9 | 1921 MB |

Standard deviations are <= 0.5 t/s in generation and 1-15 t/s in `ngl 0` prompt processing (high on the smallest models).

### Reading the results
- **The default `-ngl 0` is right:** the weights are on the processor but Vulkan speeds up prompt processing (1.5-3.4x versus CPU); generation is close to the best. Full GPU (`ngl 99`) is the slowest in generation on six of the seven models (30 -> 16 t/s on Qwen3-0.6B). The exception is gemma-3-1b (9.8 / 8.7 t/s); if benchmark history exists, the fit recommendation uses the measured direction.
- **Usable size:** 0.5-0.6B models are comfortable for chat at 20-30 t/s; 1-1.7B models at 7-16 t/s.
- **Instability:** in an earlier run (b11299), Qwen3-1.7B `ngl 99` left llama-bench hung in `drm_syncobj_array_wait_timeout` for over an hour; it completed on b11316. With OpenCode, Llama-3.2-1B `-ngl 999`, 65k context, llama-server crashed with `vk::DeviceLostError` (the GPU was reset) on a ~7.6k-token prompt. Full offload with a long prompt is not reliable on this GPU; tool preparation now uses `-ngl 0`.

### OpenCode (Llama-3.2-1B, `-ngl 0`, 65,536 context, q4_0 KV, FA, 1 slot)
`harness launch opencode --prepare --headless -- run "read the contents of the file a.txt…"`:

| Step | Time |
|---|---|
| Model load + title request (564 tokens) | ~21 s (prompt 61 t/s) |
| Main prompt 7,582 tokens (OpenCode 1.18 `run`; not ~29k) | 225 s (33.7 t/s; 68 t/s at 2k, 41 t/s at 6k) |
| First response generation | 41 tokens, 21 s (1.9 t/s) |
| Post-tool round (from cache, 108 tokens) | 3.6 s prompt + 12 s generation |
| Total | 286 s |

The model called the `read` tool (with partly corrupted arguments) but wrote the tool description instead of the file contents. **Assessment:** OpenCode works on this hardware, but the first response of every new session takes ~4 min, and a 1B model is inadequate for agent work. A proposed warning is on the [ROADMAP](ROADMAP.md).

### Harness Studio itself
| State | RAM | CPU |
|---|---|---|
| `harness serve`, no model (debug) | 47 MB, 5 threads | 0% (60 s) |
| UI open, server off (debug + `tauri dev`) | application 250 MB + WebKitWebProcess 369 MB + WebKitNetworkProcess 81 MB | 1.6% (60 s, all processes) |
| In the tray, after the page was released (debug) | application 217 MB + WebKitNetworkProcess 64 MB | — |
| llama-server, Qwen2.5-0.5B, 4k, `-ngl 33` | 143 MB (RSS) | — |

The release build was not measured; the debug application process is noticeably larger than release (on Windows, release is ~170 MiB in total).

# Concurrent Requests (Slots)

Decision and rationale: [ARCHITECTURE §5.4d](ARCHITECTURE.md). The question: llama-server's default (`-np` auto = 4 slots, unified KV), or 1 slot?

## Method
- RX 7900 GRE 16 GB, Vulkan, managed runtime b11312 (copied to a temporary directory), `-dev Vulkan0 -ngl 99 -c 32768 -fa on -fit off -cram 1024 -ctxcp 4` (Harness's memory defaults), KV f16. Configurations: no `-np` (auto: 4 slots, unified), `-np 1`, `-np 2 --kv-unified`.
- Requests are `/completion`, temperature 0, `ignore_eos`, 200 tokens of generation, a different ~1.2-1.4k-token prompt on each request (so the prefix cache does not help). Single request: the average of 2 measurements. Concurrent: 4 requests at once; total speed = 800 tokens / wall-clock time, latency = the request's total time.
- VRAM: the process's dedicated GPU memory (PDH `GPU Process Memory`), after loading and after the concurrent round. KV and checkpoint sizes from the server log (`-lv 4`).
- The GPU was shared with other workloads: a measurement was started only when another process's GPU compute engine usage (PDH `GPU Engine ... engtype_compute`) was near zero; if another llama process or GPU usage was seen during a measurement, the result was discarded and repeated.

## Results

| Model (architecture) | Slots | VRAM loaded -> end of round | KV (full + window) | Single request generation / prompt t/s | 4 concurrent: total t/s | 4 concurrent: latency min / avg / max |
|---|---:|---:|---|---:|---:|---|
| Qwen3-4B Q4_K_M (full attention) | 4 (auto) | 7229 -> 7274 MiB | 4608 MiB | 133.3 / 3099 | **184.2** | 4.3 / 4.3 / 4.3 s |
| | 2 (unified) | 7229 -> 7274 | 4608 | 133.9 / 3174 | 120.4 | 2.6 / 4.9 / 6.6 |
| | 1 | 7229 -> 7274 | 4608 | 134.5 / 3158 | 102.6 | 1.9 / 4.9 / 7.8 |
| gpt-oss-20b MXFP4 (window 128) | 4 (auto) | 12010 -> 12146 | 768 + 24 | 140.1 / 3205 | **175.9** | 4.5 / 4.5 / 4.5 |
| | 2 (unified) | 12004 -> 12141 | 768 + 18 | 131.1 / 3067 | 119.1 | 2.9 / 4.8 / 6.7 |
| | 1 | 12004 -> 12040 | 768 + 18 | 129.9 / 3038 | 95.9 | 2.0 / 5.3 / 8.3 |
| Gemma-4-12B Q4_K_M (window 1024, 40/48 layers) | 4 (auto) | **9303** -> 9894 | 512 + **1440** | 46.7 / 881 | **54.4** | 14.6 / 14.7 / 14.7 |
| | 2 (unified) | 8660 -> 8892 | 512 + 800 | 46.9 / 875 | 40.9 | 8.6 / 14.1 / 19.6 |
| | 1 | **8338** -> 8473 | 512 + 480 | 46.7 / 860 | 32.6 | 6.2 / 15.3 / 24.5 |

- **Single-request speed is independent of the slot count** (the difference is within measurement noise): an extra slot slows nothing down when idle.
- **Total speed with 4 concurrent requests** with 4 slots versus 1 slot is 1.8x on Qwen3-4B, 1.8x on gpt-oss and 1.7x on Gemma-4-12B; the duration of the longest-waiting request goes 7.8 -> 4.3 s, 8.3 -> 4.5 s, 24.5 -> 14.7 s. Average latency is about the same (waiting in the queue and generating slowly balance each other); with 1 slot, the first request finishes fastest.
- **Memory:** on the full-attention model (Qwen3-4B) and the small-window gpt-oss, the slots make no VRAM difference, or a few MiB. On Gemma-4-12B the window cache grows with slots: 4 slots is **+960 MiB** of KV versus 1 slot (measured process: +965 MiB), 2 slots +320 MiB. The size per checkpoint is independent of the slot (Gemma 256.6 MiB, gpt-oss 3.0 MiB); the total checkpoint memory is at most `--ctx-checkpoints` (Harness 4) per used slot, i.e. 4x with 4 slots.
- The memory estimate (`vram.rs`) was corrected against these measurements: Gemma 4's window layers have 8 KV heads × 256, its full-attention layers 1 × 512; the estimate gives 512 + 1440 MiB at 4 slots and 512 + 480 MiB at 1 slot (`slots::tests::gemma4_swa_cache_matches_real_loads`, logs `tests/data/b11312-vulkan-gemma4-12b-c32k-np{1,4}.load-log.txt`).

## When the Shared Pool Overflows

Qwen3-4B, `-c 8192`, two concurrent prompts of ~4.4k and ~5.4k tokens (larger than the total context):

| | Request 1 | Request 2 |
|---|---|---|
| Auto (4 slots, unified 8192) | **500 "Context size has been exceeded."** (3.2 s) | **500, the same error** (3.2 s) |
| `-np 1` | OK, 4.3 s | OK (waited in the queue), 9.3 s |

When two requests that each fit alone together exceed the pool, llama-server drops both; with 1 slot, the second one just waits. This is why the Auto rule has the condition "at least 8k tokens for every concurrent request / 32k for agents".

## Reading the Results
- Cheap slot (full attention, small window): 4 slots are free and give ~1.8x total speed on concurrent requests; the limit is the context pool, not memory.
- Expensive slot (Gemma's 1024 window, hybrid models' state): each slot costs hundreds of MiB of VRAM + checkpoint RAM. In single-user chat, 1 slot saves ~1 GiB; for agents' parallel requests, 2 slots (+320 MiB) bring the longest-waiting request from 24.5 to 19.6 s.

## Limits
- A single machine, Vulkan; for CUDA see the [section below](#concurrent-requests-cuda); on Metal the batched generation gain may differ (a reverse effect at 32k context on Metal was reported in llama.cpp #29335).
- Qwen3.5/3.6 hybrid models were not measured: the 27B Q4 does not fully fit on this card; the per-slot state (~150 MiB) was computed from the GGUF header.
- 4 concurrent requests in a single round; a short prompt (1.2-1.4k). With long agent prompts the prompt-processing share grows, and concurrent prompt processing shares the batch too.

---

# KV Cache Precision

**Policy update:** on CUDA, Metal and ROCm, the Saving step, both automatic and manual, is the matched q4_0/q4_0; Vulkan's measured q8_0/q4_0 step is kept. Details are in the backend sections.

The question: in which type should conversation memory (the KV cache, `--cache-type-k/-v`) be stored, and when should it be lowered? The goal: the fewest resources without degrading quality. The policy applied: [ARCHITECTURE.md §5.4c](ARCHITECTURE.md#54c-kv-cache-precision-kv_policyrs). All numbers were measured on the test machine (Windows 11, RX 7900 GRE 16 GB, 32 GB RAM) with a copy of the managed runtime: llama.cpp **b11312** (0c1e57098) Vulkan, `-dev Vulkan0` (iGPU excluded), `-ngl 99 -fa on`. This build turns on Hadamard rotation (PR #21038) by itself when K/V is quantized (`attn_rot_k = 1` in the log).

## Research Summary

- **Types:** `f32, f16, bf16, q8_0, q4_0, q4_1, iq4_nl, q5_0, q5_1`; default f16 ([common/arg.cpp](https://github.com/ggml-org/llama.cpp/blob/master/common/arg.cpp); verified with the runtime `--help`).
- **Flash attention:** required for quantized **V**, not for K. Without FA, V is stored transposed, so block quantization cannot be done. `-fa auto` (the default since PR #15434) turns FA off if the device does not support it. If V is quantized, llama.cpp stops with "quantized V cache was requested, but this requires Flash Attention" ([llama-context.cpp](https://github.com/ggml-org/llama.cpp/blob/master/src/llama-context.cpp)).
- **K is more sensitive than V:** PPL differences in [PR #7412](https://github.com/ggml-org/llama.cpp/pull/7412): q8_0/q8_0 +0.005; f16/q4_0 (V only) +0.022; q4_0/q4_0 +0.21. The author notes that K is much more sensitive. [Discussion #5932](https://github.com/ggml-org/llama.cpp/discussions/5932): Qwen2 degrades with q4_0, and K q8_0 + V q4_0 fixes it. The measurement of the author who added KV quantization to Ollama: q8_0 +0.002-0.05, q4_0 +0.2-0.25 PPL ([smcleod.net](https://smcleod.net/2024/12/bringing-k/v-context-quantisation-to-ollama/)).
- **Rotation:** [PR #21038](https://github.com/ggml-org/llama.cpp/pull/21038) (April 2026) rotates Q/K/V during quantization and largely reduces the loss of low-bit KV (gpt-oss-20b on AIME25 goes from 2% to 21.7% with q4_0 KV). Measurements from before this PR overstate today's loss.
- **Backend:**
  - In the default CUDA build, the FA kernel exists only for matched pairs (`q4_0-q4_0;q8_0-q8_0;f16-f16;bf16-bf16`). A mixed pair (q8_0/q4_0) runs FA **on the CPU** without `GGML_CUDA_FA_QUANTS=all` ([docs/build.md](https://github.com/ggml-org/llama.cpp/blob/master/docs/build.md), [#28633](https://github.com/ggml-org/llama.cpp/issues/28633)).
  - On ROCm too, mixed pairs fall onto a slow path ([#22411](https://github.com/ggml-org/llama.cpp/discussions/22411)).
  - On Vulkan, according to the measurement below, a mixed pair stays on the GPU; its speed is in the same class as matched pairs.
- **Model classes:**
  - Qwen2/2.5 carry outlier channels in K. KIVI's suggestion to quantize keys per channel comes from this ([arXiv 2402.02750](https://arxiv.org/abs/2402.02750)).
  - gpt-oss (attention sink) is sensitive to KV quantization.
  - On models with a sliding window (Gemma, gpt-oss), the SWA cache is already small; the gain comes from the full layers.
  - MLA models (DeepSeek) do not keep V separately; the only effective setting is `-ctk`.
- **Ollama:** `OLLAMA_KV_CACHE_TYPE` takes `f16` (default), `q8_0` or `q4_0` and applies to all models. FA is required; on an architecture where FA is not supported it silently returns to f16 ([docs.ollama.com/faq](https://docs.ollama.com/faq), [#13337](https://github.com/ollama/ollama/issues/13337)). On the test machine, the user had chosen q8_0.
- **LM Studio:** K and V types are separate in the load settings (`llamaKCacheQuantizationType`, `llamaVCacheQuantizationType`); FA is required for V ([lmstudio.ai docs](https://lmstudio.ai/docs/typescript/api-reference/llm-load-model-config)).
- Neither Ollama nor LM Studio makes an automatic choice based on model and memory.
- **Needle-in-a-haystack at long context:** smart quantization methods (KIVI, [KVQuant](https://www.stat.berkeley.edu/~mmahoney/pubs/neurips-2024-kvquant.pdf)) preserve NIAH. No published NIAH/RULER measurement was found for llama.cpp's plain block quantization; so it was measured below.

## Method

- **Quality:** `llama-perplexity` with the wikitext-2 raw test, `-c 4096 --chunks 20` (40,960 scored tokens).
  - First `--kl-divergence-base` with f16/f16, then each configuration with `--kl-divergence`.
  - Mean KLD, 99% KLD and the "same top token" rate are reported.
  - A repeat of f16/f16 was taken as the noise floor (KLD 0).
- **Needle-in-a-haystack at long context:**
  - Haystack: real chunks of ~16k and ~31k tokens from the wikitext-2 test text.
  - Four "special magic number" needles (RULER MK-NIAH format). The queried needle is at 10/30/50/70/90% depth, and the other three are distractors at other depths.
  - To count as correct, the 7-digit number must appear exactly in the answer.
  - Settings: `llama-server -c 33792 -np 1`, temperature 0, prompt cache off. The same text and needles in every configuration.
  - Number of trials (per depth and length): Qwen3-4B 3, gpt-oss-20b 2 (`reasoning_effort: low`).
- **Speed:** `llama-bench -p 2048 -n 128 -d 0,16384 -r 5`. Because the GPU was shared with other work, a second pass was made; f16 ran both at the start and the end. The table shows the second pass, and the f16 row has the range of the two runs.
- **Memory:** KV size was computed from the GGUF shape; it comes out exactly the same as the load log. Process GPU memory was read with Windows PDH `GPU Process Memory\Dedicated Usage`, after the 31k request.
- **Models:**
  - Qwen3-4B-Instruct-2507 Q4_K_M: dense, 8 KV heads × 128.
  - Qwen2.5-Coder-14B-Instruct Q4_K_M: dense, Qwen2.5 K outliers.
  - gpt-oss-20b MXFP4: MoE, sliding window + sink, 8 KV heads × 64.

## Quality (KL divergence, relative to f16 KV)

| Model | K / V | PPL | Mean KLD | 99% KLD | Same top token |
|---|---|---|---|---|---|
| Qwen3-4B | f16 / f16 | 8.705 | 0 | 0.00004 | 100% |
| | q8_0 / f16 | 8.706 | 0.00075 | 0.0052 | 98.7% |
| | f16 / q8_0 | 8.704 | 0.00063 | 0.0045 | 98.8% |
| | **q8_0 / q8_0** | 8.706 | **0.00071** | 0.0051 | 98.7% |
| | **q8_0 / q4_0** | 8.699 | **0.0028** | 0.022 | 97.6% |
| | q4_0 / q4_0 | 8.836 | 0.0236 | 0.226 | 93.3% |
| Qwen2.5-Coder-14B | f16 / f16 | 6.859 | 0 | 0.00004 | 100% |
| | q8_0 / f16 | 6.859 | 0.00028 | 0.0019 | 99.2% |
| | f16 / q8_0 | 6.858 | 0.00026 | 0.0018 | 99.2% |
| | **q8_0 / q8_0** | 6.860 | **0.00028** | 0.0020 | 99.1% |
| | **q8_0 / q4_0** | 6.862 | **0.0013** | 0.011 | 98.3% |
| | q4_0 / q4_0 | 6.907 | 0.0071 | 0.054 | 96.1% |
| gpt-oss-20b | f16 / f16 | 1769* | 0 | 0.00003 | 100% |
| | q8_0 / f16 | 1759 | 0.046 | 0.72 | 88.2% |
| | f16 / q8_0 | 1772 | 0.038 | 0.62 | 89.8% |
| | **q8_0 / q8_0** | 1758 | **0.046** | 0.75 | 88.5% |
| | **q8_0 / q4_0** | 1643 | **0.083** | 1.16 | 83.1% |
| | q4_0 / q4_0 | 1986 | 0.299 | 2.34 | 63.2% |

\* gpt-oss on raw text (without the chat template) is far outside its training distribution (PPL 1769). So KLD is exaggerated as an absolute value; the ordering among configurations is still meaningful.

## Needle in a Haystack at Long Context (correct / trials)

| Model | K / V | ~16k (16,536 / 16,351 tokens) | ~31k (31,505 / 30,643 tokens) |
|---|---|---|---|
| Qwen3-4B | f16 / f16 | 15/15 | 15/15 |
| | q8_0 / q8_0 | 15/15 | 15/15 |
| | q8_0 / q4_0 | 15/15 | 15/15 |
| | q4_0 / q4_0 | 15/15 | 15/15 |
| gpt-oss-20b | f16 / f16 | 9/10 | 10/10 |
| | q8_0 / q8_0 | 9/10 | 10/10 |
| | q8_0 / q4_0 | 9/10 | 10/10 |
| | q4_0 / q4_0 | 8/10 | 10/10 |

- The trial gpt-oss missed (30% depth, the same text) is the same in every type, including f16. q4_0/q4_0 misses one more trial.
- Single-needle retrieval does not separate the types at these context lengths; KLD shows the quality difference.

## Speed (t/s, `llama-bench`, Vulkan)

| Model | K / V | prompt 2048 | prompt 2048 @16k | generation 128 | generation @16k |
|---|---|---|---|---|---|
| Qwen3-4B | f16 / f16 | 2439-3041 | 699-701 | 87-149 | 85 |
| | q8_0 / f16 | 2842 | 871 | 144 | 85 |
| | q8_0 / q8_0 | 2843 | 856 | 143 | 91 |
| | q8_0 / q4_0 | 2894 | 817 | 140 | 92 |
| | q4_0 / q4_0 | 2978 | 840 | 142 | 93 |
| | f16 / q8_0 | 2899 | 957 | 146 | 74 |
| Qwen2.5-Coder-14B | f16 / f16 | 917-923 | 284 | 51 | 35 |
| | q8_0 / f16 | 909 | 412 | 50 | 39 |
| | q8_0 / q8_0 | 899 | 413 | 50 | 37 |
| | q8_0 / q4_0 | 900 | 390 | 49 | 37 |
| | q4_0 / q4_0 | 900 | 392 | 49 | 38 |
| | f16 / q8_0 | 919 | 433 | 50 | 33 |
| gpt-oss-20b | f16 / f16 | 3449-3453 | 1861-1864 | 147 | 129 |
| | q8_0 / f16 | 3398 | 1684 | 143 | 127 |
| | q8_0 / q8_0 | 3396 | 1749 | 141 | 127 |
| | q8_0 / q4_0 | 3339 | 1595 | 141 | 128 |
| | q4_0 / q4_0 | 3327 | 1599 | 142 | 128 |
| | f16 / q8_0 | 3407 | 1770 | 143 | 129 |

- **Prompt processing at short context:** within 1-3% of f16 for every type. Qwen3's two f16 runs varied between 2439 and 3041 among themselves (the GPU was shared).
- **At 16k depth, dense models:** quantized KV is **faster** in prompt processing. Qwen2.5-14B 284 -> 413 (+45%), Qwen3-4B 700 -> 856 (+22%). On this build, the f16 FA path stays slow at long KV.
- **At 16k depth, gpt-oss** (64-dim heads, sliding window): quantized KV is slower. q8_0 -6%, K q8_0 + V q4_0 -14%, K-only -10%.
- **Generation:** the same at short context. At 16k, the same or faster for most types; only f16 K + q8_0 V is consistently slow (-6% to -13%).
- The earlier finding that "q8_0 slows prompt processing by 10-25%" (b11236, Hermes3) was not repeated on dense models on this build.
- A mixed pair (q8_0 / q4_0) runs on the GPU on Vulkan; there is no fallback to the CPU.

## Memory

KV cache (1 slot, MiB). The calculation is exactly the same as the b11312 load log: Qwen3-4B 128k q8_0, log 9792, calculation 9792.

| Model | Context | f16 | q8_0 / f16 | q8_0 / q8_0 | q8_0 / q4_0 | q4_0 / q4_0 |
|---|---|---|---|---|---|---|
| Qwen3-4B | 32k | 4608 | 3528 | 2448 | 1872 | 1296 |
| | 128k | 18,432 | 14,112 | 9792 | 7488 | 5184 |
| Qwen2.5-Coder-14B | 32k | 6144 | 4704 | 3264 | 2496 | 1728 |
| | 128k | 24,576 | 18,816 | 13,056 | 9984 | 6912 |
| gpt-oss-20b | 32k | 786 | 602 | 418 | 320 | 221 |
| | 128k | 3090 | 2366 | 1642 | 1256 | 869 |

Process memory (`-c 33792`, MiB):

| Measurement | Model | f16 | q8_0/q8_0 | q8_0/q4_0 | q4_0/q4_0 |
|---|---|---|---|---|---|
| GPU memory, after the 31k request | Qwen3-4B | 7437 | 5337 | 4614 | 4018 |
| | gpt-oss-20b | 12,065 | 11,746 | 11,584 | 11,482 |
| Private memory, at load time | Qwen3-4B | 7514 | 5282 | 4687 | 4121 |

Private memory tracks GPU memory because the AMD Vulkan driver writes VRAM to commit too (see the "Inference Memory" section above).

**Real end-to-end test** (`kv_policy_e2e_qwen3_4b_128k`): Qwen3-4B, 131,072 context. The automatic policy chose q8_0/q8_0 and `--flash-attn on`. KV came to 9792 MiB (18,432 MiB at f16 would not have fit); process VRAM 12,519 / 16,368 MiB.

## Decision

1. **The default stays f16; automatic lowering only when it does not fit.** If memory suffices, f16 is the best quality; there is no speed difference at short context, and on gpt-oss f16 is faster at long context. As long as the allocated KV stays within 90% of the budget, the memory gained by shrinking would go to nothing else.
2. **The first step is q8_0 / q8_0.**
   - On dense models KLD is 0.0003-0.0007; the same top token 98.7-99.1%.
   - No loss in retrieval; prompt processing is faster at long context.
   - On gpt-oss, KLD is at a measurable level (0.046) but retrieval is the same as f16.
3. **The second step is K q8_0 + V q4_0 ("Saving"), not q4_0 / q4_0.**
   - On the same models KLD is 3.5-8.5x lower: Qwen3 0.0028 / 0.0236; Qwen2.5 0.0013 / 0.0071; gpt-oss 0.083 / 0.299.
   - The memory difference between the two is about 13% of f16.
   - A mixed pair is fast on Vulkan. On the default CUDA build, mixed pairs are converted to f16 on every token and become 27-38% slower ([CUDA section](#cuda)): on CUDA the automatic ladder consists only of matched pairs (f16 -> q8_0/q8_0 -> q4_0/q4_0). On Metal, mixed pairs also slow the prompt by 87-90% ([Metal section](#metal)); the ladder is the same on Metal.
4. **K q8_0 + V f16 (K-only) is not used when FA is available.**
   - KLD is the same as q8_0 / q8_0 (0.00075 / 0.00071; gpt-oss 0.046 / 0.046).
   - It saves half as much (23% / 47%) and is slower on gpt-oss.
   - It remains as the only step only when flash attention cannot be enabled (when V cannot be quantized).
5. **Limits:** the key is never lowered to 4 bits automatically; V is never quantized without FA; the user's choice and custom arguments always take precedence.

## Limits

- A single machine, a single GPU (RDNA3, Vulkan). ROCm, CUDA and Metal appear only in the sections below ([ROCm](#rocm), [CUDA](#cuda), [Metal](#metal)); the speed note on the Dashboard shows a number on Vulkan, ROCm, CUDA and Metal (the "at most" value, gpt-oss 16k).
- During measurement the GPU was shared with other workloads. In the speed table, fluctuations of up to ~20% were seen (Qwen3 f16 short prompt 2439-3041). Quality and retrieval results are not affected by this.
- KLD was measured with 40,960 tokens and 4096 context; not measured at 64k and above.
- The retrieval test has a single question and goes up to 31k. Multi-step reasoning and tool-call accuracy were not measured.
- gpt-oss's KLD was measured on raw text without the chat template; the absolute value is exaggerated.
- There was no Gemma family on this machine; sliding-window behavior was measured only with gpt-oss.


## ROCm

The question: how fast is the official ROCm build on this machine, and does the KV policy (mixed K/V pairs) apply on ROCm too? The same card (RX 7900 GRE 16 GB), llama.cpp **b11320** (b8f96c3e8), `-ngl 99 -fa 1`, `-dev ROCm1` / `-dev Vulkan0` (iGPU excluded). ROCm: the official `win-rocm-10.0-x64` archive; the archive has no `hipblas.dll`, so `libhipblas.dll` from the HIP SDK 7.1 on the machine was copied under the name `hipblas.dll` and the SDK `bin` folder was added to PATH (Harness's managed install cannot do this, see the [ROADMAP](ROADMAP.md)). Vulkan: Harness's managed b11320 Vulkan install. The GPU was shared with other workloads.

**Backend comparison** (depth 0, f16 KV, 3 repeats, t/s):

| Model | ROCm pp512 | Vulkan pp512 | ROCm tg128 | Vulkan tg128 |
|---|---|---|---|---|
| Qwen3-4B Q4_K_M | 4568 | 3522 | 127 | 143 |
| gpt-oss-20b MXFP4 | 2294 | 2627 | 98 | 143 |
| Qwen2.5-Coder-14B Q4_K_M | 1347 | 992 | 46 | 51 |

On dense models ROCm is 35-36% faster in prompt processing and 10-11% slower in generation; on gpt-oss (MoE, MXFP4) it is behind in both. Qwen3-4B launched from Harness (definition `load.runtime`) generated 124 t/s.

**KV type, Qwen3-4B** (`-ctk/-ctv`, 2 repeats, t/s; percentages relative to f16):

| Measurement | f16 | q8_0/f16 | q8_0/q8_0 | q8_0/q4_0 | q4_0/q4_0 |
|---|---|---|---|---|---|
| pp512 | 4608 | 4432 (-4%) | 4389 (-5%) | 4396 (-5%) | 4492 (-3%) |
| pp512 @ 16k | 1707 | 1676 (-2%) | 1642 (-4%) | 1676 (-2%) | 1672 (-2%) |
| tg @ 16k (5 repeats, separate run) | 70 | 56 (-20%) | 76 (+9%) | 47 (-33%) | 73 (+4%) |
| same run, Vulkan tg @ 16k | 84 | 82 | 89 | 91 | 92 |

**gpt-oss-20b** (2 repeats): pp512 @ 16k f16 2109, q8_0/q8_0 2377, q8_0/q4_0 2331, q4_0/q4_0 2362 (faster than f16); tg @ 16k f16 105, q8_0/f16 83 (-21%), q8_0/q8_0 98 (-7%), q8_0/q4_0 81 (-22%), q4_0/q4_0 96 (-8%). The f16 pp512 at depth 0 (2328) is an outlier (other types ~3500); shared GPU.

Results:
- On ROCm, **prompt processing** is at most 5% behind f16 at every KV step (`kv_policy::prompt_slowdown_pct` is 5 for ROCm).
- On ROCm, **generation** with mixed pairs (q8_0/f16, q8_0/q4_0) falls onto a slow path at long context: -20% to -33% at 16k. Matched pairs (q8_0/q8_0, q4_0/q4_0) are at f16 speed. This is consistent with upstream's notes on mixed pairs ([#22411](https://github.com/ggml-org/llama.cpp/discussions/22411)); there is no effect on Vulkan.
- **Decision: matched pair.** The ROCm automatic ladder is now the same as CUDA/Metal: f16/f16 -> q8_0/q8_0 -> q4_0/q4_0. The Saving choice on the Dashboard is also q4_0/q4_0 on these three backends (KV about 28%); lower precision on K can reduce long-context accuracy. ROCm's Compact prompt slowdown estimate is at most 5%; for a manually chosen mixed pair there is a warning of 20-33% for generation. Explicit K/V settings are preserved. On Vulkan, f16/f16 -> q8_0/q8_0 -> q8_0/q4_0 is unchanged; if flash attention is off, only the q8_0/f16 step can be used on all backends. No new GPU measurement was made.
- CUDA and Metal were measured too (below).


## CUDA

The question: how does the KV policy (mixed K/V pairs) behave on the default CUDA build? An earlier assumption was that "a mixed pair may drop FA to the CPU".

**Environment:** NVIDIA GeForce RTX 3050 Ti Laptop GPU 4 GB (cc 8.6) + Intel Iris Xe iGPU (Optimus; the dGPU does not drive the display, 12 MiB idle), driver 616.92, CUDA 13.4, Windows. llama.cpp **b11327**, the official `llama-b11327-bin-win-cuda-13.4-x64.zip` + the cudart zip (`cudart64_13`, `cublas64_13`, `cublasLt64_13` only from the cudart zip). Model Qwen3-4B-Instruct-2507-Q4_K_M (2.32 GiB). Command: `llama-bench -fa 1 -ngl 99 -p 512 -n 128 -d 0,8192,16384 -r 3`.

> **Driver note:** the 616.92 Studio driver crashed this Optimus laptop with a TDR while the GPU was idle (nvlddmkm Event ID 153, `LKD_0x141_Tdr:6_IMAGE_nvlddmkm.sys_Ampere`, a black screen and once a system freeze). The measurements completed without problems with this driver; the crashes are independent of the llama.cpp load. The results have not been repeated with an older driver version.

**KV type** (pp512 / tg128, t/s):

| K/V | d=0 | d=8192 | d=16384 |
|---|---|---|---|
| f16/f16 | 2278 / 62.06 | 1534 / 44.27 | 135 / 9.86 (VRAM overflowed) |
| q8_0/f16 | 2300 / 60.63 | 1488 / 32.47 | 97 / 1.74 (VRAM overflowed) |
| q8_0/q8_0 | 2236 / 60.26 | 1441 / 39.81 | 1058 / 29.88 |
| q8_0/q4_0 | 2230 / 59.09 | 1450 / 28.24 | 1066 / 18.48 |
| q4_0/q4_0 | 2240 / 60.36 | 1459 / 39.53 | 1071 / 29.68 |

- f16/f16 at 16k: the model 2.32 GiB + KV ~2.3 GiB exceeds 4 GB; WDDM writes the overflow to system memory (this is not a fallback to the CPU).
- q8_0/f16 at 16k: the temporary memory for converting to f16 overflows VRAM (tg 1.74 t/s).

**Findings:**
- **Mixed pairs do not fall back to the CPU.** In the default CUDA build, the FA vector kernel is not compiled for mixed pairs: llama.cpp converts K and V to f16 on every token and computes that way. This is slow and needs a temporary f16 buffer. The following line is printed to stderr (for q8_0-f16 and q8_0-q4_0; not for matched pairs):
  `W ggml_cuda_flash_attn_ext_vec: no FlashAttention vector kernel compiled for K/V types q8_0-f16, converting K and V to f16 instead (slow). Add "q8_0-f16" to GGML_CUDA_FA_QUANTS to compile it.`
- The build option is now `GGML_CUDA_FA_QUANTS` (it takes a list); the official Windows archives do not include mixed pairs.
- **Prompt processing is not affected** (at depth 8192 at most 6% behind f16: 1441-1488 / 1534). **Generation** with mixed pairs is 27-38% slower at depth (d=8192: q8_0/f16 32.5 and q8_0/q4_0 28.2 t/s, f16 44.3; d=16384: q8_0/q4_0 18.5, q8_0/q8_0 29.9). Matched pairs are ~10% behind f16 at d=8192 (39.5-39.8 / 44.3 t/s) and are the only fast path at 16k, where f16 overflows (29.7-29.9 t/s).

**Result and implementation:**
- On CUDA the "Auto" ladder is matched pairs only: f16/f16 -> q8_0/q8_0 -> q4_0/q4_0 (`kv_policy::CUDA_LADDER`; the new step `KvLevel::Compact`). ROCm has also moved to the same matched-pair ladder; Vulkan is unchanged. A mixed pair is used only if the user explicitly chooses it; the memory estimate adds the conversion buffer in that case (`kv_policy::cuda_convert_buffer_bytes`: the f16 size of the quantized sides of one layer; for Qwen3-4B at 16k, 64 MiB for q8_0/q4_0 and 32 MiB for q8_0/f16).
- The line above in the server log is recognized (`capabilities::verify::kv_fa_fallback`): a localized warning is dropped into the server log once per run ("Use a matched pair"), and the Dashboard's "Effective Values" strip shows "Slow KV pair".
- `prompt_slowdown_pct` for CUDA: q8_0/f16 3%, q8_0/q8_0 6%, q8_0/q4_0 6%, q4_0/q4_0 5% (d=8192, worst case).
- The memory estimate fits this card: 3847 MiB for q8_0 16k (measured usage 3813), 3935 MiB for q4_0 32k (3905); f16 16k is estimated at 4927 MiB and q8_0 32k at 5087 MiB, so the two cases that exceed 4 GB are foreseen in advance (`cuda_4gb_card_avoids_the_overflowing_combinations`). Because the automatic choice tries not to exceed 90% of the budget (3686 MiB), it picks q4_0/q4_0 at 16k instead of q8_0 (3847 MiB); the measurement shows q8_0 also runs at f16 speed at 16k, so this is conservative for this card. Anyone can choose "Balanced" manually.

## Metal

The question: how does the KV policy (mixed K/V pairs) behave with Metal on Apple Silicon?

**Environment:** MacBook Air M1 (8 cores, 8 GB unified memory; the Metal share is MTL0 5461 MiB per `--list-devices`), macOS 27.0.1. llama.cpp **b11344**, the official `llama-b11344-bin-macos-arm64.tar.gz` (the version Harness manages). Model Qwen3-4B-Instruct-2507-Q4_K_M (2.32 GiB, unsloth). Command: `llama-bench -ngl 99 -fa 1 -d 4096 -p 512 -n 128 -r 1 -ctk <K> -ctv <V>`. Because a single pair at d=8192 took ~10 minutes on 8 GB, the measurement was made at depth 4096 with a single repeat; Harness open, server off.

**KV type** (pp512 / tg128 @ d=4096, t/s):

| K/V | pp512 | tg128 | relative to f16 |
|---|---|---|---|
| f16/f16 | 53.31 | 7.56 | — |
| q8_0/f16 | 7.02 | 3.25 | prompt -87%, generation -57% |
| q8_0/q8_0 | 59.23 | 6.81 | prompt +11%, generation -10% |
| q8_0/q4_0 | 5.43 | 3.01 | prompt -90%, generation -60% |
| q4_0/q4_0 | 60.45 | 5.91 | prompt +13%, generation -22% |

Extra: f16/f16 at d=8192, pp512 44.84 ± 3.08, tg128 6.50 ± 0.12 (r=2).

**Findings:**
- **Mixed pairs fall onto a slow path on Metal:** both prompt processing (87-90%) and generation (57-60%) are far behind. Unlike on CUDA, prompt processing is affected too; there is no warning line in stderr as there is on CUDA.
- **Matched pairs are fast:** q8_0/q8_0 and q4_0/q4_0 are faster than f16 in prompt processing (fewer memory reads) and 10-22% behind in generation.

**Result and implementation:**
- On Metal (Apple Silicon: a unified memory budget, not the integrated GPU share), the "Auto" ladder is the same as CUDA's: f16/f16 -> q8_0/q8_0 -> q4_0/q4_0 (`kv_policy::CUDA_LADDER`). Intel/AMD integrated GPUs (Vulkan, shared RAM) stay on the old ladder.
- `prompt_slowdown_pct` for Metal: q8_0/f16 87%, q8_0/q8_0 0%, q8_0/q4_0 90%, q4_0/q4_0 0%; the generation loss is `METAL_MIXED_GENERATION_SLOWDOWN_PCT` (57, 60). The Dashboard Saving choice became q4_0/q4_0 for CUDA/Metal/ROCm; the 4-bit tooltip explains the key precision loss.
- Limit: a single machine (M1, 8 GB), a single model, a single repeat, 4k depth. On larger Apple Silicon (M2/M3/M4 Pro/Max) the absolute numbers will differ; since a mixed pair falling onto a slow path depends on the build's kernel support, the direction is expected to be the same.

## Concurrent Requests and Memory Estimate, Metal

The same M1 8 GB, managed runtime b11347 (macos-arm64), Qwen3-4B-Instruct-2507 Q4_K_M. Command: `llama-server -ngl 99 -fa on -c 16384 -np <slots>` (KV f16). Each request is `/completion`, 898 prompt tokens (a different prefix per request), `n_predict 256`, `ignore_eos`, `cache_prompt false`. "Total" = all generated tokens / wall-clock time (including prompt processing); "per request" = the average of `timings.predicted_per_second`.

| Slots | Requests | Total t/s | Per request t/s | Wall-clock |
|---:|---:|---:|---:|---:|
| 1 | 1 | 4.34 | 5.95 | 58.9 s |
| 1 | 4 (queued) | 4.17 | 5.73 | 245.4 s |
| 2 | 2 | **5.49** | 3.83 | 93.3 s |
| 4 | 2 | 4.02 | 2.56 | 127.4 s |
| 4 | 4 | 5.02 | 1.86 | 204.0 s |

- **The slot gain is small on M1:** in the best case 2 slots give 1.26x versus a single slot (on CUDA 4 slots give 2.3x, on Vulkan 1.8x). The 8-core GPU saturates in compute; every request generated together slows proportionally. 4 slots came out slower than 2 slots with 2 requests (a single measurement; thermal throttling was not ruled out).
- Single-request generation at 16k f16 KV is 5.95 t/s (9.3 t/s had been measured in the Playground at 4k): long KV and the 898-token prompt slow generation.
- The default (`-np` auto, unified KV) does no harm on M1 but the gain is small too; not measured on larger Apple Silicon (more GPU cores).

**Memory estimate versus actual** (buffer sizes in the `-lv 4` log; Metal = model + KV + compute buffer, total = Metal + CPU buffers (304 MiB CPU_Mapped embedding, output, CPU compute)):

| Context / KV | Dashboard estimate (VRAM) | Actual Metal | Actual total |
|---|---:|---:|---:|
| 4k f16 | 3.11 GB | 3028 MiB (2.96 GiB) | 3349 MiB |
| 8k f16 | 3.68 GB | 3624 MiB (3.54 GiB) | 3945 MiB |
| 16k q8_0/q8_0 (automatic choice) | 3.76 GB | 3722 MiB (3.63 GiB) | 4043 MiB |
| 16k f16 (manual) | — | 4816 MiB | 5137 MiB |

- The estimate is ~4% conservative at every context (0.13-0.15 GiB above actual); there is no overflow risk. At 16k the Dashboard picks q8_0/q8_0 for KV by itself (f16 would fit in the 5.3 GB budget but leaves no margin).
- `phys_footprint` (4k: 671 MB) does not show real usage because it does not count `mmap`ped weights; the log was used as the measure.

## Concurrent Requests (CUDA)

The same machine and build (b11327). Command: `llama-server -ngl 99 -fa on -c <context> -np <slots> -ctk/-ctv <type>`. Each request is `/completion`, 796 prompt tokens, `n_predict 256`, `ignore_eos`, `cache_prompt false`. "Total" = all generated tokens / wall-clock time; "per request" = the average of `timings.predicted_per_second`. VRAM: `nvidia-smi` (it stays at ~3.9 GB on WDDM; overflow shows up only as slowdown).

| Context / KV | Slots | Requests | Total t/s | Per request t/s | VRAM MiB |
|---|---:|---:|---:|---:|---:|
| 32k q4_0/q4_0 | 1 | 1 | 51.08 | 55.57 | 3905 |
| | 1 | 4 (queued) | 51.45 | 55.39 | |
| | 2 | 2 | 88.15 | 48.93 | 3901 |
| | 4 | 2 | 88.42 | 49.08 | |
| | 4 | 4 | **117.69** | 34.52 | 3893 |
| 32k q8_0/q8_0 (overflows) | 1 | 1 | 24.94 | 31.31 | 3921 |
| | 2 | 2 | 31.10 | 19.16 | |
| | 4 | 4 | 36.17 | 11.64 | |
| 16k q8_0/q8_0 | 1 | 1 | 51.05 | 55.25 | 3813 |
| | 2 | 2 | 88.76 | 49.34 | |
| | 4 | 2 | 88.73 | 49.28 | |
| | 4 | 4 | **117.53** | 34.54 | 3801 |
| 16k f16/f16 (overflows) | 1 | 1 | 35.54 | 45.02 | |
| | 2 | 2 | 49.29 | 32.35 | |
| | 4 | 4 | 60.73 | 20.99 | 3921 |

- **The gain is real when KV fits in VRAM:** 1.73x total speed with 2 slots, 2.3x with 4 slots (51 -> 118 t/s). In configurations that overflow (32k q8_0, 16k f16), the slot gain is halved and absolute speed drops too (36 and 61 t/s with 4 slots).
- Requests beyond the slots (np 1, 4 requests) queue; total speed is the same as a single request's.
- In this trial the server started with `kv_unified=false`: `n_ctx_slot = context / slots` (32k and 4 slots: 8k per request). Harness sends `--kv-unified` for `-np N > 1` (`slots::plan_for_launch`), so requests can use the whole pool; this table was not measured separately with the unified pool (the total KV memory is the same).
- Recommendation on a 4 GB card: q4_0/q4_0 for 32k, q8_0/q8_0 for 16k.

## Full GPU Load and the `-ngl` Recommendation, CUDA

The same machine, driver 616.92 (Studio), the managed runtime the application installed (b11327 **win-cuda-12.4** + cudart 12.4). Launched from the application: Qwen3-4B Q4_K_M, 16k, `-np 2 --kv-unified`, `--flash-attn on`. 2 concurrent requests (`/completion`, `n_predict` 400-800, `ignore_eos`). VRAM: PDH `GPU Process Memory` (process).

| `-ngl` | KV | Dedicated VRAM MiB | Shared MiB | Per request t/s | Total t/s |
|---:|---|---:|---:|---:|---:|
| 33/37 (old recommendation) | q8_0/q8_0 | 3475 | – | 35.1 | 70.7 |
| 37/37 | q8_0/q8_0 | 3801 | 106 | **54.9** | ~110 |
| 37/37 (Auto KV) | q4_0/q4_0 | 3225 | 106 | 57.6 (single request) | – |

- The full load uses 96% of the card (3801 / 3962 MiB, DXGI) but does not overflow into shared memory: shared use is the same as at -ngl 33 (106 MB). Speed is 56% higher per request.
- The recommendation was being cut at the card's **90%** with `vram::recommend_gpu_layers` (CUDA's `--list-devices` "free" value, 3295 MiB, does not enter the recommendation; only the device list and the gateway memory check). The measurement record works (`vram_measurements`, at the default log level only the process total) and corrects the recommendation: when a full load was measured with Auto KV, the recommendation went 33 -> 37 (by choosing q4_0/q4_0). The 3801 MiB measured manually with q8_0 still exceeds 90%, so the recommendation stayed at 36.
- **Change:** if there is a measurement taken at full GPU with the same model, context and KV types, the full load corrected with it is accepted up to 97% (`vram::measured_full_offload`; the slot count may differ). In the form, with q8_0, 16k, -ngl 33, it now says "Recommended -ngl: 37". If there is no measurement or it exceeds 97%, the old rule applies; there is no blind raising (WDDM silently overflows into shared memory).

## Speculative Decoding (Draft Model), CUDA

The same machine (RTX 3050 Ti Laptop 4 GB, driver 616.92), managed runtime b11333 win-cuda-12.4. Target Qwen3-4B-Instruct-2507 Q4_K_M, draft Qwen3-0.6B Q8_0 (the official `Qwen/Qwen3-0.6B-GGUF`, 610 MiB). The same tokenizer family; it matches the `library::draft` recommendation. llama-server directly: `-ngl 99 -np 1 -fa on --spec-type draft-simple`, draft `-md … -ngld 99`. Three prompts (code, explanation, translation + list), each 400 tokens, `temperature 0`. VRAM: PDH `GPU Process Memory` (process). Acceptance: `timings.draft_n_accepted / draft_n`.

| Context | Configuration | Dedicated / shared MiB | t/s (avg.) | Acceptance |
|---|---|---:|---:|---:|
| 2k | no draft | 2833 / 92 | 59.7 | – |
| 2k | draft on GPU, `--spec-draft-n-max 3` (default) | 3695 / 100 | 56.4 | 51% |
| 2k | draft on GPU, n-max 2 | 3695 / 100 | 54.9 | 57% |
| 4k | no draft | 3123 / 94 | 59.8 | – |
| 4k | draft on GPU, n-max 3 | 3923 / **392** | 46.2 | 51% |
| 4k | draft on GPU, n-max 8 | 3923 / 392 | 34.2 | 29% |
| 8k | no draft | 3703 / 98 | 59.4 | – |
| 8k | draft on GPU | does not start | – | – |
| 8k | draft on CPU (`-ngld 0`) | 3775 / 114 | 18.0 | 51% |

- **The draft gives no gain on this card.** The 4B is already 60 t/s. Each step of the 0.6B draft (3 tokens) takes nearly as long as one target step, and acceptance is 51% (Qwen3-0.6B is a thinking model, the target is 2507 Instruct; mean accepted length 2.4-2.7). When nothing overflows (2k) it is break-even (-6%), when it overflows (4k) -23%, and with the draft on the CPU -70%. A long draft (n-max 8) lowers acceptance to 29%. A gain is expected in the case where the target is slow, a large model + a small draft: on a Windows/Vulkan machine, Qwen2.5-Coder-14B + 0.5B was measured at 46 -> 89 t/s. With Harness's definition (`draft_model_path`, `draft_gpu_layers` 99), on a short and predictable response ("count from 1 to 10"), 21/21 draft tokens were accepted, 61.8 t/s.
- **Memory:** the draft = its weights + its own KV cache in the target's context (b11333 has no separate draft context, `-cd` was removed) + a compute buffer. Measured addition: 862 MiB at 2k, 1098 MiB at 4k. At 8k, with the target fully on the GPU, the draft does not fit (even with q8_0 KV), and llama.cpp reports this not as a memory error but as `error loading model: invalid vector subscript` / `failed to load draft model`. It starts with `-ngld 0` or `-ngl 30` on the target.
- **The estimate was corrected:** the VRAM estimate did not count the draft at all (`FitInput` had no draft). Now there is `FitInput.draft` (path, size, `-ngld`, KV types). The draft is computed as a separate model with the target's context cells and slots (`vram::add_draft`); `MemoryEstimate.draft_gpu_bytes` / `draft_host_bytes` are in the result. The recommended `-ngl` is computed with the budget minus the draft's share. With real files: the estimate is 860 MiB at 2k (measured 862) and 1086 MiB at 4k (measured 1098). The recommended target `-ngl` is 36 at 2k and 32 at 4k (37 with no draft).

## `--disable-gpu`: CPU During Use

An isolated debug instance (`tauri build --debug --no-bundle`, separate `HARNESS_DATA_DIR` and `WEBVIEW2_USER_DATA_FOLDER`), two llama-servers (qwen2.5 0.5B, 4k context, Vulkan), window 1000×661 CSS px, display scale 115%. CPU is from the processor-time difference of the application process and all WebView2 child processes (excluding llama-server), relative to one core. Scrolling: with CDP, a 120 px wheel event every 50 ms (direction changes in 5 steps), 15 s. Streaming response: a new chat in the Playground, ~1460 tokens, a 6 s window; CPU-seconds per 1000 tokens. The GPU-on round was done twice; the first overlapped with a cargo build running at the same time, and the table has the clean round (the streaming values of the overlapping round were 2.13-2.24).

| Scenario | GPU on | `--disable-gpu` |
|---|---|---|
| Dashboard, two servers, live charts, idle (20 s) | 1.3% | 0.9% |
| Scrolling (2 × 15 s) | 19.7% / 15.5% | 22.3% / 20.3% |
| Streaming response, CPU-s / 1000 tokens (3 rounds) | 2.17 / 1.68 / 1.77 | 2.64 / 2.03 / 1.83 |
| GPU process | 112 MiB | 52 MiB |
| Renderer | 134 MiB | 187 MiB |

- **Decision: not enabled.** The idle gain (42 MiB in the earlier measurement) is cancelled during use by the renderer growing; CPU rises 10-20% in scrolling and streaming responses. The difference is within noise when idle with charts.
- Limit: a single machine (RX 7900 GRE, Windows 11), a debug build, short windows; the streaming response measurement depends on model speed (~370 t/s).

## Gateway JIT/TTL/LRU, CUDA

Machine: RTX 3050 Ti Laptop 4 GB (WDDM, driver 617.42) + Iris Xe, llama.cpp b11456 CUDA 12 (`cudart64_12`), a Harness debug build, a separate data directory; gateway 11635. The client is Node `fetch` (an external client like OpenCode), each model definition with defaults (context 4096, `-ngl` automatic). Driven by a Node test script; the t/s variability between runs is high (laptop power/clock state; the 1.5B, 3B and Llama values are from the `timings` field of short 8-token responses, the 0.5B from 120 tokens), ranges are from 5 runs.

| Model (definition) | JIT load (`x-harness-load-ms`) | Generation (t/s) | VRAM while loaded (nvidia-smi, total) | Eviction (`x-harness-evicted`) |
|---|---|---|---|---|
| Qwen2.5-0.5B Q4_K_M | 3.1-7.0 s | 215-242 (stream 232) | 553 MiB | on a Llama Q6 request: `llama-3b-q6` (oldest) |
| Qwen2.5-1.5B Q4_K_M (with 0.5B loaded) | 3.9-5.0 s | 39-74 | 1762 MiB (with 0.5B) | on a Llama Q6 request: `qwen-1-5b,qwen-3b` |
| Qwen2.5-3B Q4_K_M (with 0.5B+1.5B loaded) | 3.3-8.0 s | 38-56 | 3368 MiB (with 1.5B) | `qwen-0-5b` (oldest, made room for 3B) |
| Llama-3.2-3B Q6_K (with all loaded) | 8.9-12.2 s | 44-52 | 3131 MiB (alone) | `qwen-1-5b,qwen-3b` (two models, oldest first) |
| 0.5B + 3B together (with the fix) | 3.2 s (3B) | 51 | 2712 MiB | none |

- **The estimate is very close to the measurement:** for Llama Q6_K the gateway said "about 3130 MiB"; the loaded measurement was 3131 MiB. For 3B, 2158 MiB estimated, 2159 MiB measured alone.
- With the **pinned 3B** loaded, a Llama Q6 request gets `503 insufficient_memory`: "needs about 3130 MiB, 1932 MiB free; loaded and not evictable: qwen-3b" (`Retry-After: 30`). The idle 0.5B was evicted in this trial but it still was not enough.
- **TTL:** 15 s in the definition; unloaded 16.1 s after the last request, VRAM 553 -> 0 MiB. The "Unload" button unloaded the 1.5B immediately (state "Not loaded").
- **e2e durations (3B, ctx 4096-8192):** `e2e_jit_stream_ttl_embed_rerank` JIT 4.2 s, stream 172 chunks (first 25 ms, total 2.24 s ≈ 77 t/s), in the memory pressure test the 3B -> 1.5B load 5.6 s and the `gw-big-a` eviction; `admission_e2e` 2.06 s and 4.23 s with a request that waited 2049 ms in the queue; `split_e2e` two streams of 160 tokens 2.32 s (~69 t/s); `serve_mode::e2e` draft (0.5B -> 3B) 22 draft tokens / 21 accepted, 86.7 t/s; embedding 768 dimensions. Peak VRAM during tests: gateway `e2e` 3908 MiB (oom test, spill with `--fit`), others 2159-2627 MiB.
- **RAG (nomic-embed Q4_K_M + bge-reranker-v2-m3 Q4_K_M):** indexing 138 chunks (embed JIT included) 6.1 s, rerank (JIT included) 7.0 s. The 3B answer 5.1 s, correct content.
- **Did not fit (measured):** the Qwen3-4B-Instruct-2507 Q4_K_M chat model at 8192 context gets `503 insufficient_memory` even after embed+rerank were evicted (estimate ~4.1 GB). Llama Q6_K 8192 context, with 3B loaded: "needs about 3639 MiB, 4091 MiB free" (including the 512 MiB margin when memory awareness is on).
- **Playground (3B, gateway SSE):** ~75 t/s on the UI counter; first token 60-680 ms (prompt cache); after the `exec_shell_command` approval the second round is 0.7 s.
- **Finding (the version without the fix):** because of the 3 s device report cache, with the 0.5B loaded a 3B request wrongly got `503 (1512 MiB free)` and evicted the 0.5B; with the fix, 200 and no eviction.

## whisper.cpp CUDA and an LLM on the Same Card

RTX 3050 Ti Laptop 4 GB, driver 617.42, Windows 11, whisper.cpp b5454 CUDA 12.4 (the application's downloader), 11 s of real speech (`samples/jfk.wav`), `whisper-server` `/inference`, a single request, 4 consecutive requests (first / warm). VRAM: dedicated memory per process (nvidia-smi total, 0 MiB idle). Other builds were running on the machine during measurement (CPU times may have been affected; model load times depend on the file cache: between 3 s and 114 s).

| Model | CUDA first / warm (s) | CPU warm (s) | Speedup | VRAM (MiB) | `whisper_vram_need` before / after (MiB) |
|---|---|---|---|---|---|
| ggml-base.en | 0.50 / 0.32 | 1.10 | 3.5x | 411 | 333 / 461 |
| ggml-small.en | 0.71 / 0.41 | 3.55 | 8.7x | 805 | 657 / 785 |
| ggml-medium.en | 0.75 / 0.63 | 10.8 | 17x | 1945 | 1828 / 1956 |
| ggml-large-v3-turbo-q5_0 | 3.45 / 0.48 | 14-15 | 30x | 893 | 739 / 867 |

- **The CUDA 11.8 archive does not use the GPU:** `cublas64_11.dll` is missing from `whisper-bin-win-cuda-11.8.0-x64.zip`; VRAM 0 MiB, CPU speed. The 12.4 archive (675 MB) works, including cuBLAS. The selector now picks 12.4.
- **On the same card (Qwen2.5-3B Q4_K_M 16k = 2602 MiB, 0.5B = 552 MiB, whisper medium.en = 1944 MiB / turbo = 892 MiB):** with medium.en, 3B + 0.5B + whisper = 5.1 GB does not fit. In the order 3B, 0.5B, whisper request: the gateway first evicted the oldest model (3B) and whisper loaded (2498 MiB used). Then a 3B request: 0.5B and whisper were evicted (2603 MiB), then a whisper request evicted the 3B (1945 MiB). With turbo (after the fix): a 3B request evicted only the 0.5B, whisper stayed (3496 / 4096 MiB), and the whisper request took 325 ms.
- **Pinned 3B:** a whisper request gets 503 `insufficient_memory` ("needs about 1828 MiB, 1488 MiB free", nvidia-smi 2603 MiB), 3B stays in place, 503 in 506 ms.
- **In the application (Playground > Speech, file drop):** medium.en first load + transcription 4.7 s (file in cache; cold 52 s), warm 0.59 s; the small.en switch 7.6 s; two models together 2750 MiB.

## NVIDIA GPU Selection and VRAM Telemetry

RTX 3050 Ti Laptop 4 GB (WDDM, driver 617.42) + Intel Iris Xe (Optimus), Windows 11. llama.cpp b11445 downloaded by the application itself (win-cuda-13.4 + cudart, win-vulkan), a separate data folder, driven by a Node test script. All servers were launched from the application, f16 KV, FA on, `-np 1`; speed is `/completion` 128 tokens.

**Device list.** CUDA: only `CUDA0` RTX (Iris Xe is not visible). Vulkan: `Vulkan0` Iris Xe (7.9 GB shared), `Vulkan1` RTX. Automatic selection picks the RTX on both runtimes, `--main-gpu 0` (the order among discrete devices; DXGI 1 -> 0). Iris Xe: on CUDA the option is disabled, and on manual selection "The selected GPU can't be used with this runtime: Intel(R) Iris(R) Xe Graphics (adapter 0)"; the same on Vulkan (llama.cpp does not load the iGPU when a discrete GPU exists).

**VRAM: server share (PDH `GPU Process Memory`) and nvidia-smi `memory.used` (MiB).**

| Model | Context | -ngl | nvidia-smi | PDH server share | PDH card total | t/s |
|---|---:|---:|---:|---:|---:|---:|
| Qwen2.5-0.5B Q4_K_M | 8k | 99 | 605 | 605 | 609 | 199-229 |
| Qwen2.5-0.5B Q4_K_M | 16k | 99 | 709 | 709 | 713 | 229 |
| Qwen2.5-1.5B Q4_K_M | 8k | 99 | 1325 | 1325 | 1329 | 85-123 |
| Qwen2.5-1.5B Q4_K_M | 16k | 99 | 1557 | 1557 | 1561 | 118-125 |
| Qwen2.5-3B Q4_K_M | 8k | 99 | 2307 | 2307 | 2311 | 75-77 |
| Qwen2.5-3B Q4_K_M | 16k | 99 | 2603 | 2603 | 2607 | 75-77 |
| Llama-3.2-3B Q6_K | 4k | 99 (29/29) | 3131 | 3131 | 3135 | 60-61 |
| Llama-3.2-3B Q6_K, `-np 2` | 16k | 99 | 3901 | 3902 | 3906 | 56 (card full: estimate 4393-4487 MiB, recommendation -ngl 23-24) |
| Qwen2.5-0.5B, Vulkan | 8k | 99 | 522 | 521 | 525 | 191-195 |

- The server share is the same MiB as nvidia-smi; the card total is +4 MiB (+0.1-0.6%, other processes). The speed fluctuation is from other processes building at the same time.
- **NVML (WDDM, 617.42):** `nvmlDeviceGetComputeRunningProcesses_v3` succeeds, but the llama-server entry has `usedGpuMemory = NVML_VALUE_NOT_AVAILABLE`; nvidia-smi shows `[N/A]`. On Windows the process share comes from PDH.
- **Two servers:** two Qwen2.5-0.5B 8k (11792 and 11793) together, 605 + 605 MiB as separate shares on the RTX bar; Iris Xe 0. When both stop, all shares are 0.
- **-ngl recommendation (cold estimate):** Llama-3.2-3B Q6_K 4k: estimate 3187, measured 3131 MiB, full-load recommendation 29/29 ("full_gpu"). Qwen2.5-0.5B 8k: estimate 521, measured 605 MiB (the CUDA context is missing in a small model, harmless). Llama `-np 2` 16k filled the card (3901 / 3962 MiB, "oom_risk"), yet it ran at 56 t/s; WDDM moves the excess into shared memory.
- **False positive:** the "could not fit into free device memory" warning came from the "already set by user" line llama-server writes on every launch when `-ngl` is given (even on a 605 MiB model); fixed.

**14B (Qwen2.5-14B Q4_K_M, 8k) partial offload only** (a full load does not fit in 4 GB; the Dashboard warning "Estimated 5.75-6.9 GB needed, dedicated VRAM 3.87 GB", recommended -ngl 17-29, the recommendation drops after each measurement):

| -ngl (/48) | nvidia-smi MiB | t/s |
|---:|---:|---:|
| 29 | 3899 | 4.0 |
| 25 | 3883 | 4.5 |
| 21 | 3747 | 4.2 |
| 17 | 3815 | 4.6 |
| 13 | 3171 | 4.1 |
| 9 | 2429 | 3.9 |

- At 17 layers and above the card is full (3.75-3.9 GB, ceiling 3962) and speed stays at ~4 t/s with partial offload; the layer count does not change speed noticeably. Prompt processing is 8-12 t/s.


## Hybrid Models: Per-Slot State, CUDA

The question: on hybrid models such as Qwen3.5 (gated delta net layers + attention in every 4th layer), do `vram.rs`'s recurrent state (`recurrent_bytes_per_seq`) and KV estimates agree with a real CUDA load; what are the per-slot cost and the concurrent gain?

**Environment:** RTX 3050 Ti Laptop 4 GB (WDDM, driver 617.42, CUDA 13.4) + Intel Iris Xe (Optimus), 15.8 GB RAM, Windows 11. llama.cpp **b11456** (the official CUDA 12 archive, the managed runtime installed by the application's own downloader), `-ngl 99 -fa on -lv 4`, KV f16. Models are unsloth Q4_K_M (0.8B 508 MB, 2B 1.19 GB, 4B 2.55 GB) and 4B Q8_0 (4.17 GB). Estimate: `vram::estimate_raw` (from the GGUF header, no measurement correction); actual: the llama-server `llama_kv_cache` / `llama_memory_recurrent` / `sched_reserve` buffer lines and the `nvidia-smi` total (relative to 0 MiB at idle). GGUF headers: 0.8B 24 layers (6 attention + 18 recurrent), 2B 24 (6 + 18), 4B 32 (8 + 24); `ssm.conv_kernel 4`, `state_size 128`, `inner_size` 2048 / 2048 / 4096, `group_count 16`, `full_attention_interval 4`, attention head width 256.

### Estimate and Measured (MiB)

| Model | -np | Context | KV estimate / measured | State (RS) estimate / measured | Compute estimate / measured | Total VRAM estimate / nvidia-smi | Difference |
|---|---:|---:|---:|---:|---:|---:|---:|
| 0.8B | 1 | 4k | 48 / 48 | 19.3 / 19.27 | 32 / 37 | 757 / 695 | +8.9% |
| 0.8B | 1 | 16k | 192 / 192 | 19.3 / 19.27 | 44 / 49 | 913 / 851 | +7.3% |
| 0.8B | 1 | 32k | 384 / 384 | 19.3 / 19.27 | 60 / 65 | 1121 / 1059 | +5.9% |
| 0.8B | 2 | 16k | 192 / 192 | 38.5 / 38.53 | 44 / 50 | 932 / 873 | +6.8% |
| 0.8B | 4 | 4k | 48 / 48 | 77.1 / 77.06 | 32 / 48 | 815 / 765 | +6.5% |
| 0.8B | 4 | 16k | 192 / 192 | 77.1 / 77.06 | 44 / 60 | 971 / 921 | +5.4% |
| 0.8B | 4 | 32k | 384 / 384 | 77.1 / 77.06 | 60 / 76 | 1179 / 1129 | +4.4% |
| 2B | 1 | 4k | 48 / 48 | 19.3 / 19.27 | 52 / 56 | 1491 / 1409 | +5.8% |
| 2B | 1 | 16k | 192 / 192 | 19.3 / 19.27 | 64 / 68 | 1647 / 1585 | +3.9% |
| 2B | 1 | 32k | 384 / 384 | 19.3 / 19.27 | 80 / 84 | 1855 / 1793 | +3.4% |
| 2B | 2 | 16k | 192 / 192 | 38.5 / 38.53 | 64 / 68 | 1666 / 1605 | +3.8% |
| 2B | 4 | 4k | 48 / 48 | 77.1 / 77.06 | 52 / 56 | 1549 / 1487 | +4.1% |
| 2B | 4 | 16k | 192 / 192 | 77.1 / 77.06 | 64 / 68 | 1705 / 1643 | +3.7% |
| 2B | 4 | 32k | 384 / 384 | 77.1 / 77.06 | 80 / 84 | 1913 / 1851 | +3.3% |
| 4B | 1 | 4k | 128 / 128 | 50.2 / 50.25 | 76 / 78 | 3018 / 2957 | +2.1% |
| 4B | 1 | 16k | 512 / 512 | 50.2 / 50.25 | 88 / 90 | 3414 / 3353 | +1.8% |
| 4B | 1 | 32k | 1024 / 1024 | 50.2 / 50.25 | 104 / 106 | 3942 / 3881 | +1.6% |
| 4B | 2 | 16k | 512 / 512 | 100.5 / 100.50 | 88 / 90 | 3465 / 3403 | +1.8% |
| 4B | 4 | 4k | 128 / 128 | 201.0 / 201.00 | 76 / 78 | 3169 / 3107 | +2.0% |
| 4B | 4 | 16k | 512 / 512 | 201.0 / 201.00 | 88 / 90 | 3565 / 3503 | +1.8% |
| 4B | 4 | 32k | 1024 / 1024 | 201.0 / 201.00 | 104 / 106 | 4093 / 3919 | +4.4% (card full) |

- **The recurrent state and KV estimates are correct:** KV is byte-for-byte identical in all measured configurations (np 1/2/4, 4k/16k/32k); the state (RS) is within ±0.2% (per slot 19.27 / 19.27 / 50.25 MiB; linear with `-np`). The unified pool (`--kv-unified`) and a separate pool give the same KV and RS (np 4, 16k); the only difference is the compute buffer (+12 MiB unified at 4 slots on 0.8B).
- **The total VRAM estimate is 1.6-9% high (on the safe side):** the constant ~150 MiB runtime margin is proportionally larger on small models. The compute buffer is at most 16 MiB short (0.8B, 4 slots); too small to affect the estimate.
- **Context checkpoint** (in main memory): one checkpoint is as large as the recurrent state of one full slot (log: 2B 19.266 MiB, 4B 50.251 MiB; about 2 per prompt, at most 4 per slot with `--ctx-checkpoints 4`). The estimate (`checkpoint_bytes` = state x slots x 4) is the same. The Dashboard's "each extra request ~96 MB" matches 19.27 (state) + 4 x 19.27 (checkpoints) = 96.3 MiB for the 2B.

### Concurrent Requests (total t/s)

Command `-ngl 99 -fa on -c 16384 -np N [--kv-unified]`. Each request is `/completion`, 492 prompt tokens, `n_predict 256`, `ignore_eos`, `cache_prompt false`; the first request was sent separately as a warm-up. "Total" = generated tokens / wall-clock time (including prompt processing). Gain = the ratio to the same model's `-np 1`, single-request value.

| Model | -np | Pool | Streams | Total t/s | Per request t/s | Gain |
|---|---:|---|---:|---:|---:|---:|
| 0.8B | 1 | separate | 1 / 4 (queued) | 156.3 / 149.1 | 166.2 / 160.0 | 1.00x / 0.95x |
| | 2 | unified | 2 | 239.0 | 134.8 | 1.53x |
| | 4 | unified | 2 / 4 | 232.0 / 256.8 | 127.0 / 74.3 | 1.48x / **1.64x** |
| | 4 | separate | 4 | 273.1 | 76.9 | 1.75x |
| 2B | 1 | separate | 1 / 4 (queued) | 95.8 / 94.2 | 101.2 / 100.0 | 1.00x / 0.98x |
| | 2 | unified | 2 | 155.9 | 85.1 | 1.63x |
| | 4 | unified | 2 / 4 | 152.8 / 198.2 | 83.9 / 56.8 | 1.60x / **2.07x** |
| | 4 | separate | 4 | 206.2 | 57.8 | 2.15x |
| 4B | 1 | separate | 1 / 4 (queued) | 46.7 / 46.2 | 49.7 / 49.7 | 1.00x / 0.99x |
| | 2 | unified | 2 | 75.8 | 43.1 | 1.62x |
| | 4 | unified | 2 / 4 | 78.1 / 100.3 | 43.1 / 29.4 | 1.67x / **2.15x** |
| | 4 | separate | 4 | 103.3 | 29.7 | 2.21x |

- The gain is of the same order as with Qwen2.5-3B (the "Concurrent Requests, CUDA" section: 1.73x with 2 slots, 2.3x with 4 slots): 1.5-1.6x with 2 slots, 1.6-2.2x with 4 slots. As the model gets smaller the gain decreases (on the 0.8B, the per-request kernel launch overhead dominates).
- The recurrent state and KV always fit in VRAM on this card (4B np 4 16k: 3503 MiB), so the slot count caused no overflow-driven slowdown. 4B np 4 32k filled the card (3919 MiB).
- The unified pool is 3-6% slower than a separate pool (the total KV is the same). Harness continues to choose the unified pool for `-np N > 1`: requests are not limited to `-c / N`.

### Configurations That Do Not Fit

| Configuration | Measured | Note |
|---|---|---|
| 4B Q8_0, `-c 4096`, `-ngl 99` | `CUDA0 model buffer` 4264 MiB (larger than the card's 4096 MiB); nvidia-smi stuck at 3901 MiB; **11.2 t/s** (Q4_K_M: 46.7) | **Did not fit:** WDDM writes the excess to shared memory; it runs without crashing but 4x slower. Estimate: model 4275 (measured 4264, +0.2%), total 4679 MiB. At 16k nvidia-smi is also 3885 MiB. |
| 4B Q8_0, `-c 4096`, `-ngl 24` | nvidia-smi 3549 MiB, **15.0 t/s** | Partial load raises speed by 34%, still a third of Q4_K_M. |
| 2B Q4_K_M, `-c 262144`, f16 KV | KV 3072 MiB + model 1211 + compute 308; nvidia-smi 3899 MiB; 43.4 t/s (single request, total) | Did not fit (spill). The Dashboard shows "Memory May Not Be Enough: estimated 5.08 GB needed, dedicated VRAM 3.87 GB" and asks "Start Anyway". The buffer total (4.5 GiB) + ~150 MiB ≈ 5.0 GB: consistent with the estimate. |
| 2B Q4_K_M, `-c 262144`, f16, `-ngl 16` (the Dashboard's recommendation) | nvidia-smi 3901 MiB; 1024 MiB of KV on the CPU, **the compute buffer ballooned to 1052 MiB**; 44.7 t/s | The recommendation still fills the card (partial load alone does not make it fit at this context). |
| Qwen3.6-35B-A3B (MoE) partial offload | **Could not be tried** | The smallest suitable file `UD-Q4_K_M` is 22.13 GB (20.6 GiB) > the machine's 15.8 GB RAM; free RAM during the measurement was 0.7-1.1 GB (other processes). Estimated from the 32 MiB header: 40 layers (10 attention + 30 recurrent), np 1 state 62.8 MiB, np 4 251 MiB, KV 16k 320 MiB (np 1) / 571 MiB (np 4 unified). Not measured. |

### Bug Found: Conversion Buffer of Quantized KV with 256-Wide Heads at Long Context

The measurement showed the estimate's only real deviation. With Qwen3.5 (head width 256) and `q8_0`/`q4_0` KV, the flash attention kernel converts the quantized K and V to f16 layer by layer (CUDA's vector kernel goes up to 128). The compute buffer grows by the f16 size of one layer's K + V:

| Model, KV | Context | Compute buffer (CUDA0 MiB) | Old estimate |
|---|---:|---:|---:|
| 2B f16 | 65536 | 116 | 112 |
| 2B q8_0 | 40960 / 49152 / 57344 / 65536 | 152 / 176 / 200 / 224 | 92 / 100 / 108 / 112 |
| 2B q8_0 | 131072 / 262144 | 416 / 800 | 176 / 304 |
| 2B q4_0 | 262144 | 800 | 304 |
| 4B q4_0 | 65536 / 131072 | 380 / 700 | 140 / 204 |
| Qwen2.5-3B (head 128) q8_0 / q4_0 | 65536 | 148 / 148 (f16: 145) | 150 |

- Consequence: "Auto" KV chose q8_0 for the 2B at 262144 context and said "fits in VRAM, 3.19 GB"; in reality nvidia-smi was 3757 MiB (3.67 GiB, 15% short). Configurations at the card's limit could have received a wrong "fits" label. There is no deviation on models with head width 128.
- **Fix** (`kv_policy::cuda_convert_buffer_bytes`): on models whose key head width is greater than 128, the f16 equivalent of one layer's quantized K/V (`n_ctx x (K + V) x 2` bytes) is added to the estimate for a matched quantized pair too (not when flash attention is off or the backend is not CUDA). It is ~16-20 MiB more than measured, on the safe side. After the fix, in the application at 262144 / Auto: KV `q4_0` is chosen (KV 883 instead of 3091 MiB), estimate 2.94 GB, nvidia-smi 2989 MiB (0.7%); at 65536 / Auto: f16, 2.16 GB estimated / 2209 MiB measured.
- Other families with 256-wide heads, such as Gemma, get the same rule but were not measured; the estimate did not change on ROCm/Vulkan (measured only on CUDA).
- The `CUDA_Host compute buffer` pinned in main memory (as many KiB as the context: 272 MiB at 262144) does not enter the estimate; it stays inside `HOST_OVERHEAD`.

### Dashboard (CDP, hidden window, separate data directory)

Qwen3.5-2B Q4_K_M, Auto settings ("Start Server" from the application):

| Setting | Dashboard | Actual (log / nvidia-smi) |
|---|---|---|
| Default: context 4096, `-np` Auto | **1 slot** ("each extra request ~96 MB; 1 is enough for a single user"), GPU layers 25/25, KV f16, "Fits in VRAM, 1.4 GB" | `n_slots = 1`, 25/25, nvidia-smi 1429 MiB (1.40 GiB) |
| 16k, `-np 4` | Slots 4, unified KV on, "Fits in VRAM, 1.6 GB" | `n_slots = 4, kv_unified = true`, 1643 MiB |
| 32k, `-np 4` | "1.81 GB" | 1851 MiB (1.81 GiB) |
| 64k, Auto | f16, "2.16 GB" | 2209 MiB (2.16 GiB) |
| 262144, Auto (before the fix) | KV q8_0, "3.19 GB" fits | 3757 MiB: does not fit (15% short) |
| 262144, Auto (after the fix) | KV q4_0, "2.94 GB", log "KV 883 MiB instead of 3091" | 2989 MiB |
| 262144, KV Full (f16) | "Memory May Not Be Enough: estimated 5.08 GB, dedicated VRAM 3.87 GB", recommended -ngl 16 | buffer total ~5.0 GB; nvidia-smi stuck at 3899 MiB (spill) |

- The recommended `-np`/`-ngl`/KV are consistent with the measurement. A remaining note: at context 262144 a "Didn't fit in memory" label appears under Effective Values (the llama-server `failed to fit params to free device memory` line: because of its own 1 GiB margin); the configuration actually runs (2989 MiB). The label relays llama-server's view and does not contradict Harness's estimate.

### Limits

- A single machine (RTX 3050 Ti Laptop 4 GB, WDDM); Qwen3.5-0.8B/2B/4B, a single build (b11456). Qwen3.6-35B-A3B could not be measured (RAM).
- nvidia-smi saturates at ~3.9 GB on WDDM; configurations that overflow show up only as slowdown.
- The concurrent measurement was with a short prompt (492 tokens); with long prompts, context checkpoints (at most 4 per slot) grow main memory (see "Real RAM Growth"). Because 4 slots x ~7.5k-token prompts did not fit the 16k pool, in the checkpoint trial the 4-slot run went unanswered (token-budgeted admission is in the gateway).

- **Fresh device report cost (CUDA):** `make_room` now runs `llama-server --list-devices` on every memory decision; on this card ~1.0 s (the first call 2.7 s). JIT loads with the combined code: 0.5B 4.3 s (earlier runs 4.9-7.0), 1.5B 4.2 s, 3B 6.3 s (load only) / 3.1 s (with 0.5B loaded), Llama Q6_K 9.2 s; the variability between runs exceeds the additional ~1 s, and a separate increase could not be measured. The 16-step gateway test script passed 16/16 with the combined code.

## Out-of-the-Box Settings and Parallel Requests

Machine: RX 7900 GRE 16 GB (Vulkan, b11552); the desktop (dwm etc.) holds ~4.2 GiB while idle. Out of the box = a definition with no settings, through the gateway (an isolated data directory, a built binary). Compared with the installed version of an earlier comparison benchmark (before the full-offload fix: 4B 29/37 layers, 14B 33/49 + overflow) and an intermediate build with that fix.

| | Reference (installed, before the fix) | Intermediate build | Current |
|---|---|---|---|
| 4B decode, out of the box | 47.9 t/s (29/37 layers, 32k) | 145 t/s (37/37, 32k) | 139-145 t/s (37/37, 32k) |
| 14B decode, out of the box | 14.2 t/s, layers 33/49 + shared-memory slowdown | 46.0 t/s, 49/49, 32k q8_0/q4_0, **1.1 GiB overflowed into shared memory** | 48.7 t/s, 49/49, 16k q8_0/q4_0, no overflow (61 MiB) |
| 14B tool test (38 cases), out of the box | 5/38 | 5/38 | 35-36/38 |
| 4 concurrent streams, total t/s, out of the box | 53 (1 slot) | 139 (1 slot) | **385** (4 slots, unified pool) |
| 4 slots on (`equal4`) total t/s | 170 | - | 393 |

* **Flash attention:** when `-fa` is not given, llama.cpp uses `auto` and turns it on on Vulkan RDNA3: Qwen3-4B 32k f16, `-fa auto` 150.6 t/s, `-fa on` 148.1, `-fa off` **123.4** (and it overflows 2.1 GiB into shared memory). So "FA off" is only the `flash_attn: false` field on the effective settings screen; the default `auto` is right. The earlier measurement with a low layer count (29/37) was resolved by `full_offload_context`.
* **Overflow threshold (AMD/Windows):** Qwen2.5-Coder-14B Q4_K_M, `-ngl 99`, `-fa on`; the process's dedicated memory / the card's total usage / overflow into shared memory: 8k f16 9.8 GiB / 14.1 GiB / 49 MiB; 16k q8_0 9.9 GiB / 14.2 GiB / 57 MiB; 24k q8_0+q4_0 9.3 GiB / 13.6 GiB / **923 MiB**; 32k q8_0 9.5 GiB / 13.9 GiB / **2113 MiB**. The budget Vulkan and DXGI report (15557 MiB) does not show this; the driver overflows once the total of all processes passes ~2.1 GiB below the card. The estimate is correct (9956 MiB estimated / 9986 measured). So on Windows + a discrete AMD card, the automatic margin is at least 2200 MiB instead of `max(1 GiB, 8%)` (`vram::AMD_WINDOWS_DRIVER_RESERVE`). On NVIDIA, since 3801/3962 MiB was measured without overflow, the margin did not change.
* **Unified pool + main-memory prompt cache stall (llama.cpp b11552):** with 4 slots `--kv-unified` and `--cache-ram > 0` (default 8192, Harness's 1024), the first token of every parallel request wave is delayed by ~3.7 s (a wait before `launch_slot_`); the first wave is fast. 4 streams x 256 tokens, Qwen3-4B: `-np 4` (not unified) 395 t/s; `-np 4 --kv-unified` 172; `--kv-unified --cache-ram 0` 400; only `-c 32768` (llama-server default, unified) 172; plus `--cache-ram 0` 400; `-np 2 --kv-unified` 136-174 (TTFT 2-6 s). This is the cause of the "Harness 4 slots 170, llama-server 398" difference in the comparison: the llama-server row had run `-np 4` without unified. The fix: if there are multiple slots in the unified pool and the user did not choose, `--cache-ram 0` (`slots::runs_unified_pool`, `process.rs`). A previous prompt still stays in its slot; what is lost is only the main-memory cache across slots.
