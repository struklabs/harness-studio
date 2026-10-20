# Performance and Analytics

Türkçe: [../tr/07-performans-ve-analitik.md](../tr/07-performans-ve-analitik.md)

**What you do here:** measure model speed, watch resource use, and look into the latency or error cause of gateway requests.

The **Performance** and **Analytics** pages are in the **Monitor** group of the left menu. They are Advanced-level pages; switch to **Advanced** if you do not see them (see [Simple and Advanced levels](README.md#simple-and-advanced-interface-levels)).

## Watch Live Performance

1. Start a server and send a few chat messages.
2. In the left menu, open **Performance** under **Monitor** and choose the server from the selector at the top.
3. Look at **Overview**, **Generation Speed and Latency**, **Memory and GPU**, and **Clients** in the **Live** group.
4. When you try the same model with different load settings, compare speed and memory use.

Generation speed in `t/s` is the number of tokens produced per second. Prompt processing speed is how fast the content you send is read. Time to first token describes when the answer begins, and total time describes the whole request. The approximate time to first token on the Performance screen is taken from prompt processing time when the server does not report it separately.

GPU load and temperature are read with `nvidia-smi` on NVIDIA, and from system sensors for AMD and Intel on Linux (temperature only on Intel); on Windows and macOS these values may not appear for AMD and Intel. Do not read missing sensor data as zero usage. The server's RAM use and the app's RAM use can be shown separately.

## Manage GPU Jobs

The Dashboard's **Jobs** panel lists queued loads and active engine processes, their priority, estimated GPU memory, and queue wait. Interactive chat loads take precedence over agent tasks, followed by background embedding loads and benchmarks. Jobs at the same priority start in arrival order; running work is not preempted.

When the estimated model, context, and runtime memory do not fit, the load waits. Stop an unused model to make room, or cancel the waiting job. **Cancel** on a running engine stops that engine and interrupts requests using it. GPU memory remains reserved until its process exits. An engine with an unknown GPU memory requirement starts only when no other GPU reservation is held, and then holds the whole GPU until it exits.

Speech and image engines share this memory queue with chat engines. The gateway continues to manage speech request slots and idle unloading. Queue wait and load times are logged for comparisons; one-shot binaries use their first output as the observable load boundary. Control API clients can inspect the same queue with `GET /api/harness/v1/jobs`; this endpoint is read-only and requires read access.

OpenAI-compatible gateway requests can send `X-Harness-Priority: interactive`, `agent`, or `background` to set load priority. Playground chat uses interactive priority, other clients default to agent priority, and embeddings and benchmarks use background priority. Training engines can use the same reservation interface when they become available.

## Run a Benchmark

1. Open **Performance > Benchmark > New Measurement**.
2. Choose a local **Model**. For a first try you can use the **Quick** preset.
3. Check **Prompt sizes (pp)** and **Generation sizes (tg)**. More than one value creates different test combinations.
4. For comparisons you can pick the **GPU Layer Sweep**, **Flash-attn Comparison**, or **KV Cache Comparison** preset.
5. If another server is running, consider stopping it first; sharing the GPU lowers the measurement.
6. Press **Start Benchmark**. Read the outcome under **Results**. Use **Cancel** if you need to.
7. In the **Compare** flow, pick at least two records. Check that the model, runtime, parameters, and test sizes are suitable to compare.

Benchmarks need `llama-bench`. If it cannot be found, see the [troubleshooting table](10-troubleshooting.md#the-runtime-engine-is-missing-or-will-not-start). A benchmark is a fixed test load; it does not exactly guarantee the speed of a real chat. The most recent benchmark's best prompt and generation speeds are shown next to the gateway's real-use averages so you can compare the two.

## Analytics and the Request Log

1. In the left menu, open **Analytics** under **Monitor**. Make sure the gateway is on and **Record Gateway Requests** is enabled (it is in the **Recording and Retention Policy** section).
2. Use **Overview** and **Model and Client Breakdown** to look at requests, tokens, speed, latency, and error rates.
3. On the **Log** page use the **Model**, **Client**, **Endpoint**, and **Status** filters.
4. Select a request. Read **Timeline**, **Tokens and Speed**, **Resources**, and **Error** under **Selected Request Detail**. The timeline shows queue, load, prompt, and generation phases, and a failed tool call is diagnosed down to the likely culprit (server or template, context, model, or client).
5. If queue or load times are high, check whether the model was loading for the first time and whether other requests were running.

P50 shows the median latency, and P95 shows the latency below which the great majority of requests finished. These values are computed from the period in which raw records are kept; you cannot get the same detail from older daily totals.

Gateway requests are recorded one by one. Direct Playground and tool-generator use can add to the total token breakdown; direct calls do not have the same request detail. The total inferred from metrics for other direct use may be approximate.

On the **Recording and Retention Policy** page you can edit how long raw records are kept (the default is 30 days; older rows are rolled up into daily totals). By default the metadata recorded is time, model, client, tokens, durations, memory, and errors. Prompt and response content is not recorded by default.

If **Capture Request Content (Analytics)** is turned on in a model definition, prompts and responses passing through the gateway are written to disk as plain text. Do not turn this on for sensitive content. **Delete Analytics** cannot be undone; chats are not affected by it.

The **Tool Calls** log lets you inspect the tool name, client, result, and approval status. Denied calls are logged too.

Benchmark results depend on your model, runtime, settings, and hardware. Run benchmarks on your own machine to compare configurations. See [Features](../../en/FEATURES.md) for supported capabilities.

Next: [Settings](08-settings.md).
