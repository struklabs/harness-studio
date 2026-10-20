import { invoke } from '#lib/ipc.js';
import { listen } from '@tauri-apps/api/event';
import { onWindowActivity, windowActive } from '#lib/windowActivity.js';
import type {
  ActiveServerInfo,
  AppSettings,
  DefaultServer,
  GatewayOverview,
  LaunchConfig,
  ServerProfile,
  ServerStatus,
  ServerSummary,
  SystemHardwareInfo,
  TelemetrySnapshot,
} from '#lib/types.js';
import { t } from '#lib/i18n.svelte.js';
import { toast } from 'svelte-sonner';
import { serverErrorText } from '#lib/failure.js';
import { assignSlots } from '#lib/serverColor.js';
import { runningGatewayEndpoint } from '#lib/gateway-url.js';

export const DEFAULT_SERVER_ID = 'default';
const SELECTED_SERVER_KEY = 'harness_selected_server';

/** Telemetry pushed by the backend (`telemetry` event) for the watched server. */
interface TelemetryEvent {
  server_id: string;
  snapshot: TelemetrySnapshot;
  at_ms: number;
}

/** Samples kept for the tokens/s sparklines (one per second). */
const TPS_HISTORY = 60;

/**
 * Single source of truth for app-wide state shared by several views. Nothing here polls: the
 * backend pushes server list changes (`servers-changed`), the watched server's telemetry
 * (`telemetry`, only while the window is visible) and gateway changes (`gateway-changed`).
 */
class AppState {
  settings = $state<AppSettings | null>(null);
  hardware = $state<SystemHardwareInfo | null>(null);

  /** Every llama-server instance known to the backend (pushed on every change). */
  servers = $state<ServerSummary[]>(loadCachedServers());
  /** Saved server profiles (fetched on demand, see refreshProfiles). */
  profiles = $state<ServerProfile[]>([]);
  /** Instance the Dashboard / Playground / Connections / Performance views act on. */
  selectedServerId = $state<string>(loadSelectedServerId());
  private profilesLoaded = false;
  private serversJson = '';

  selectedProfile = $derived(this.profiles.find((p) => p.id === this.selectedServerId) ?? null);
  /**
   * The Dashboard's own server while it is not running: its saved launch (the one source of
   * truth, updated by every start), else the settings defaults. Null until first loaded.
   */
  defaultServer = $state<DefaultServer | null>(null);
  private defaultServerSig: string | null = null;
  private defaultServerSeq = 0;
  private defaultServerFirst: Promise<void> | null = null;
  /** Saved launch of the selected server when it is not running (profile or Dashboard server). */
  selectedSavedConfig = $derived<LaunchConfig | null>(
    this.selectedProfile?.config ??
      (this.selectedServerId === DEFAULT_SERVER_ID && this.defaultServer ? this.defaultServer.config : null)
  );
  /** Set by "New server" buttons outside the Dashboard; the page opens the Dashboard, which creates it. */
  newServerPending = $state(false);
  /** Tab another component wants shown (consumed by the page, which resets it to null). */
  requestedCreatePage = $state<string | null>(null);
  requestedRegistryKind = $state<'skill' | 'mcp' | 'tool' | null>(null);
  requestedModelResources = $state<string | null>(null);
  requestedConnectionPage = $state<string | null>(null);
  requestedTab = $state<string | null>(null);
  /** Search the Model Hub should run when it is shown (consumed by the Hub, which resets it to null). */
  requestedHubSearch = $state<string | null>(null);
  /** Repo (and file in it) the Model Hub should open in its drawer when shown (consumed by the Hub, then null). */
  requestedHubRepo = $state<{ id: string; file?: string } | null>(null);
  /** Playground tab another component wants shown (consumed by the Playground, then null). */
  requestedPlaygroundTab = $state<'chat' | 'embedding' | 'rerank' | 'transcription' | 'image' | 'pipeline' | null>(null);
  /** Alias to select in the tab `requestedPlaygroundTab` opens (Embedding / Rerank; consumed with it, then null). */
  requestedPlaygroundModel = $state<string | null>(null);
  /** Text to append to the chat composer's draft (consumed by the Playground, then null). */
  requestedComposerText = $state<string | null>(null);
  /** Bumped to make open overlays (the Hub's variant drawer, dialogs) close before a navigation. */
  overlayCloseTick = $state(0);
  /**
   * Opens a Playground tab from anywhere (e.g. a toast action): overlays close first, so a modal
   * drawer cannot keep covering the page, then the tab switch follows on the next task.
   */
  openPlaygroundTab(tab: 'chat' | 'embedding' | 'rerank' | 'transcription' | 'image' | 'pipeline') {
    this.overlayCloseTick++;
    setTimeout(() => {
      this.requestedPlaygroundTab = tab;
      this.requestedTab = 'playground';
    }, 0);
  }
  /** Opens the chat tab with `text` added to the draft (not sent), e.g. a Speech transcript. */
  addToChat(text: string) {
    this.requestedComposerText = text;
    this.openPlaygroundTab('chat');
  }
  /** Same for any page (e.g. the download toast's "open in Dashboard" over the Hub drawer). */
  openTab(tab: string) {
    this.overlayCloseTick++;
    setTimeout(() => {
      this.requestedTab = tab;
    }, 0);
  }
  /** Analytics view another component (the command palette) wants shown. */
  requestedAnalyticsView = $state<'overview' | 'requests' | null>(null);
  selectedServer = $derived(this.servers.find((s) => s.id === this.selectedServerId) ?? null);
  /** Status of the selected instance (not created yet = Stopped). */
  serverStatus = $derived<ServerStatus>(this.selectedServer?.status ?? 'Stopped');
  /** Connection info of the selected server while it runs (null when stopped). */
  activeServer = $derived<ActiveServerInfo | null>(this.selectedServer?.active ?? null);
  runningCount = $derived(this.servers.filter((s) => s.status === 'Running').length);

  isRunning = $derived(this.serverStatus === 'Running');
  isBusy = $derived(this.serverStatus === 'Starting' || this.serverStatus === 'Stopping');
  errorMessage = $derived(
    typeof this.serverStatus === 'object' && 'Error' in this.serverStatus ? this.serverStatus.Error : null
  );

  /** Host/port/key clients should use: the live server if any, else the configured defaults. */
  connectHost = $derived(
    this.activeServer?.connect_host ?? connectHostFor(this.selectedSavedConfig?.host ?? this.settings?.server.host)
  );
  /** Live port, else the selected server's saved port, else the default setting. */
  port = $derived(this.activeServer?.port ?? this.selectedSavedConfig?.port ?? this.settings?.server.port ?? 8080);
  /**
   * API key of the selected running server. The list only says whether one is set; the key is
   * fetched on demand (`get_server_api_key`) and only kept in memory.
   */
  apiKey = $state<string | null>(null);
  apiKeySet = $derived(this.activeServer?.api_key_set ?? false);
  private apiKeySig = '';
  /**
   * Latest telemetry per server (pushed while the window is visible): the selected server
   * always, the others while a view that shows every running server is on screen.
   */
  telemetryAll = $state.raw<Record<string, TelemetrySnapshot>>({});
  /** Recent generation speed samples (tokens/s) per server, oldest first, for sparklines. */
  tpsByServer = $state.raw<Record<string, number[]>>({});
  private tpsAt: Record<string, number> = {};
  /** Latest telemetry of the selected server. */
  telemetry = $derived<TelemetrySnapshot | null>(this.telemetryAll[this.selectedServerId] ?? null);
  /** Generation speed samples of the selected server. */
  tpsHistory = $derived<number[]>(this.tpsByServer[this.selectedServerId] ?? []);
  /** Starting/running servers (what the telemetry cards and log tags are about). */
  activeServers = $derived(this.servers.filter((s) => s.active != null));
  /** Colour slot of every starting/running server (see `serverColor`); stable while it runs. */
  serverSlots = $state.raw<Record<string, number>>({});
  /** Views on screen that show system RAM/VRAM even while no server runs. */
  private memoryViews = 0;
  /** Views on screen that show every running server, not only the selected one. */
  private allViews = 0;
  private watchKey = '';
  // Order of watch calls; starts from the clock so a reloaded page still counts past the backend's last one.
  private watchSeq = Date.now() * 1000;
  private started = false;

  /** No live server and the port is Automatic (0): the direct address is only known after a start. */
  portPending = $derived(!this.activeServer && this.port === 0);
  hostUrl = $derived(`http://${this.connectHost}:${this.port}`);
  baseUrl = $derived(`${this.hostUrl}/v1`);

  /** Gateway (managed endpoint) status; refreshed with the server list (at most every 3 s). */
  gateway = $state<GatewayOverview | null>(null);
  private gatewayAt = 0;
  private gatewaySeq = 0;
  gatewayEndpoint = $derived(runningGatewayEndpoint(this.gateway));
  gatewayRunning = $derived(this.gatewayEndpoint !== null);
  gatewayBaseUrl = $derived(this.gatewayEndpoint?.base_url ?? null);
  /**
   * Base URL (`.../v1`) for inference requests of Harness's own clients (Playground,
   * integrations): the managed endpoint while the gateway runs, else the selected server.
   * Server-specific calls (`/health`, `/metrics`, `/tools`) keep using `hostUrl`.
   */
  inferenceUrl = $derived(
    this.gatewayBaseUrl ?? this.baseUrl,
  );
  /** The gateway's root (`http://host:port`) while it runs. */
  gatewayHostUrl = $derived(this.gatewayEndpoint?.host_url ?? null);

  async refreshGateway(force = false) {
    const now = Date.now();
    if (!force && now - this.gatewayAt < 3000) return;
    this.gatewayAt = now;
    const seq = ++this.gatewaySeq;
    try {
      const gateway = await invoke<GatewayOverview>('get_gateway_status');
      if (seq === this.gatewaySeq) this.gateway = gateway;
    } catch {
      if (seq === this.gatewaySeq) this.gateway = null;
    }
  }

  setGateway(o: GatewayOverview) {
    this.gatewaySeq++;
    this.gateway = o;
    this.gatewayAt = Date.now();
  }

  /** Headers for requests to `inferenceUrl`. */
  inferenceHeaders(): Record<string, string> {
    if (!this.gatewayRunning) return this.authHeaders();
    const token = this.gateway?.client_token;
    return {
      'X-Harness-Client': 'harness-playground',
      ...(token ? { Authorization: `Bearer ${token}` } : {}),
    };
  }

  /** One `list_servers` call; status/connection of the selected server are derived from it. */
  async refreshServer() {
    try {
      await this.applyServers(await invoke<ServerSummary[]>('list_servers'));
    } catch {
      // keep the last known list
    }
  }

  /** Status of each server at the last list, to tell a crash from a start that failed. */
  private lastStatus = new Map<string, ServerSummary['status']>();

  /**
   * A server that was running and is now in Error crashed: say so wherever the user is (the
   * Dashboard shows the details), with a way there. A start that failed is shown where it was
   * started, so only Running -> Error is announced.
   */
  private announceCrashes(list: ServerSummary[]) {
    for (const s of list) {
      const before = this.lastStatus.get(s.id);
      const isError = typeof s.status === 'object' && s.status !== null && 'Error' in s.status;
      if (before === 'Running' && isError) {
        // One toast per server: a crash loop (automatic restarts) updates it instead of stacking.
        const server = s.id === DEFAULT_SERVER_ID ? t('srv.default_label') : this.serverLabel(s.id);
        toast.error(t('dash.server_crashed', { server }), {
          id: `crash-${s.id}`,
          description: serverErrorText(s) ?? undefined,
          action: {
            label: t('dash.server_crashed_open'),
            onClick: () => {
              this.selectServer(s.id);
              this.openTab('dashboard');
            },
          },
        });
      }
    }
    this.lastStatus = new Map(list.map((s) => [s.id, s.status]));
  }

  /** Takes a server list (fetched, or pushed with `servers-changed`). */
  private async applyServers(list: ServerSummary[]) {
    this.announceCrashes(list);
    const json = JSON.stringify(list);
    if (json !== this.serversJson) {
      this.serversJson = json;
      this.servers = list;
      try {
        localStorage.setItem(SERVERS_CACHE_KEY, json);
      } catch {
        // ignore
      }
    }
    this.ensureSelectionExists();
    this.serverSlots = assignSlots(
      list.filter((s) => s.active != null).map((s) => s.id),
      this.serverSlots,
    );
    this.pruneTelemetry(list);
    // Every start of the Dashboard server saves its launch: re-read it whenever that server's
    // status or running launch (port, model) changes, whoever started it (form, CLI, load).
    const def = list.find((s) => s.id === DEFAULT_SERVER_ID);
    const sig = JSON.stringify([def?.status ?? null, def?.active?.port ?? null, def?.active?.model_path ?? null]);
    if (sig !== this.defaultServerSig) {
      this.defaultServerSig = sig;
      void this.refreshDefaultServer();
    }
    this.syncTelemetryWatch();
    await this.syncApiKey();
    await this.refreshGateway();
  }

  /**
   * Subscribes to the backend's pushes (once, from the page). Returns the cleanup. While the
   * window is hidden nothing is watched; when it shows again everything is read once.
   */
  start(): () => void {
    if (this.started) return () => {};
    this.started = true;
    const offs: (() => void)[] = [];
    let disposed = false;
    const on = <T>(event: string, cb: (payload: T) => void) => {
      listen<T>(event, (e) => cb(e.payload))
        .then((fn) => (disposed ? fn() : offs.push(fn)))
        .catch(() => {});
    };
    // Pushed while hidden too (rare); applying it keeps the tray-less state current.
    on<ServerSummary[]>('servers-changed', (list) => void this.applyServers(list).catch(() => {}));
    on<TelemetryEvent>('telemetry', (e) => this.applyTelemetry(e.server_id, e.snapshot, e.at_ms));
    on<null>('gateway-changed', () => void this.refreshGateway(true));
    // The launch is saved only after the server reached Running (a moment after the status
    // push that already re-read it): read the saved launch again then.
    on<null>('last-session-changed', () => void this.refreshDefaultServer());
    offs.push(
      onWindowActivity((active) => {
        if (active) void this.refreshServer();
        this.syncTelemetryWatch();
      }),
    );
    this.syncTelemetryWatch();
    return () => {
      disposed = true;
      this.started = false;
      offs.forEach((fn) => fn());
      this.watchKey = '';
      void invoke('watch_telemetry', { serverId: null }).catch(() => {});
    };
  }

  /**
   * A view that shows system RAM/VRAM while no server runs (the Dashboard meters) is on
   * screen; returns the call that ends it. Memory is then sampled every 2 s.
   */
  watchMemory(): () => void {
    this.memoryViews++;
    this.syncTelemetryWatch();
    return () => {
      this.memoryViews--;
      this.syncTelemetryWatch();
    };
  }

  /**
   * A view that shows every running server (the Dashboard's telemetry cards) is on screen;
   * returns the call that ends it. The backend then pushes each server's changes.
   */
  watchAllServers(): () => void {
    this.allViews++;
    this.syncTelemetryWatch();
    return () => {
      this.allViews--;
      this.syncTelemetryWatch();
    };
  }

  /** Tells the backend what to push: the selected server's telemetry, only while visible. */
  private syncTelemetryWatch() {
    if (!this.started) return;
    const active = windowActive();
    const memory = this.memoryViews > 0;
    const all = this.allViews > 0;
    // A running server is always watched (title bar speed); a stopped one only for the meters.
    const wanted = active && (memory || all || this.selectedServer?.active != null);
    const key = wanted ? this.selectedServerId + '|' + memory + '|' + all : '';
    if (key === this.watchKey) return;
    this.watchKey = key;
    const seq = ++this.watchSeq;
    const id = this.selectedServerId;
    const showAll = wanted && all;
    invoke<TelemetrySnapshot | null>('watch_telemetry', { serverId: wanted ? id : null, memory, all: showAll, seq })
      .then(async (snap) => {
        if (seq !== this.watchSeq) return;
        if (snap) this.applyTelemetry(id, snap, Date.now());
        if (!showAll) return;
        const others = await invoke<{ server_id: string; snapshot: TelemetrySnapshot }[]>('get_telemetry_all');
        if (seq !== this.watchSeq) return;
        const now = Date.now();
        for (const o of others) if (o.server_id !== id) this.applyTelemetry(o.server_id, o.snapshot, now);
      })
      .catch(() => {
        this.watchKey = ''; // try again with the next change
      });
  }

  /** Forgets servers that no longer exist (their telemetry, sparkline and clock). */
  private pruneTelemetry(list: ServerSummary[]) {
    const known = new Set(list.map((s) => s.id));
    const stale = Object.keys(this.telemetryAll).filter((id) => !known.has(id) && id !== this.selectedServerId);
    if (stale.length === 0) return;
    const drop = <T>(m: Record<string, T>) => Object.fromEntries(Object.entries(m).filter(([id]) => !stale.includes(id)));
    this.telemetryAll = drop(this.telemetryAll);
    this.tpsByServer = drop(this.tpsByServer);
    for (const id of stale) delete this.tpsAt[id];
  }

  private applyTelemetry(id: string, snap: TelemetrySnapshot, atMs: number) {
    this.telemetryAll = { ...this.telemetryAll, [id]: snap };
    const running = this.servers.find((s) => s.id === id)?.status === 'Running';
    const history = this.tpsByServer[id] ?? [];
    if (!running) {
      if (history.length) this.tpsByServer = { ...this.tpsByServer, [id]: [] };
      this.tpsAt[id] = 0;
      return;
    }
    // One sample per second; a quiet stretch (nothing pushed while nothing changed) repeats
    // the last value, so the sparkline keeps its time axis.
    const at = this.tpsAt[id] ?? 0;
    const secs = at ? Math.round((atMs - at) / 1000) : 1;
    if (secs < 1 && history.length) {
      this.tpsByServer = { ...this.tpsByServer, [id]: [...history.slice(0, -1), snap.predicted_tokens_per_sec] };
      return;
    }
    const last = history.at(-1) ?? 0;
    const gap = Math.min(Math.max(secs - 1, 0), TPS_HISTORY);
    const next = [...history, ...Array<number>(gap).fill(last), snap.predicted_tokens_per_sec];
    this.tpsAt[id] = atMs;
    this.tpsByServer = {
      ...this.tpsByServer,
      [id]: next.length > TPS_HISTORY ? next.slice(next.length - TPS_HISTORY) : next,
    };
  }

  /** Fetches the selected server's key when the server (re)starts or its selection changes. */
  private async syncApiKey() {
    const active = this.activeServer;
    if (!active || !(active.api_key_required ?? active.api_key_set)) {
      this.apiKeySig = '';
      this.apiKey = null;
      return;
    }
    const sig = `${this.selectedServerId}|${active.port}|${active.model_path}|${JSON.stringify(this.serverStatus)}`;
    if (sig === this.apiKeySig && this.apiKey) return;
    this.apiKeySig = sig;
    let key: string | null = null;
    try {
      key = (await invoke<string | null>('get_server_api_key', this.serverArgs())) ?? null;
    } catch {
      key = null;
    }
    // Only the newest fetch lands (the selection may have changed meanwhile).
    if (this.apiKeySig === sig) this.apiKey = key;
  }

  /**
   * Re-reads the Dashboard server's saved launch (`get_default_server`). Several reads can be
   * in flight (status changes, a start); only the newest may land, or an older answer read
   * before a start saved its launch would show the previous port as "Saved".
   */
  refreshDefaultServer(): Promise<void> {
    const seq = ++this.defaultServerSeq;
    const p = invoke<DefaultServer>('get_default_server')
      .then((d) => {
        if (seq === this.defaultServerSeq) this.defaultServer = d;
      })
      .catch(() => {
        // keep the last known value
      });
    this.defaultServerFirst ??= p;
    return p;
  }

  /** Resolves once the Dashboard server's saved launch has been read at least once. */
  defaultServerLoaded(): Promise<void> {
    return this.defaultServerFirst ?? this.refreshDefaultServer();
  }

  async refreshProfiles() {
    try {
      this.setProfiles(await invoke<ServerProfile[]>('list_server_profiles'));
    } catch {
      // keep the last known list
    }
  }

  /**
   * Profiles made with "New server" in this session that have not been started yet: their first
   * start defines them (name, model and launch follow the form). Not persisted.
   */
  readonly unstartedProfiles = new Set<string>();

  /** Replaces the profile list (e.g. with the list returned by save/delete). */
  setProfiles(list: ServerProfile[]) {
    this.profiles = list;
    this.profilesLoaded = true;
    this.ensureSelectionExists();
  }

  selectServer(id: string) {
    this.selectedServerId = id;
    this.apiKey = null;
    this.apiKeySig = '';
    // The new server's key now, not at the next server-list push.
    void this.syncApiKey();
    this.syncTelemetryWatch();
    try {
      localStorage.setItem(SELECTED_SERVER_KEY, id);
    } catch {
      // storage unavailable
    }
  }

  /** Arguments every server-scoped command takes. */
  serverArgs(): { serverId: string } {
    return { serverId: this.selectedServerId };
  }

  /**
   * Human-readable name of an instance: its profile's name, "Default server", or the raw id.
   * Older auto-names ended in " :<port>"; the port is shown next to the name everywhere, so a
   * trailing port is dropped from the display (the saved name is left untouched).
   */
  serverLabel(id: string): string {
    if (id === DEFAULT_SERVER_ID) return t('srv.default_label');
    const name = this.profiles.find((p) => p.id === id)?.name ?? id;
    return name.replace(/\s*:\d{2,5}$/, '') || name;
  }

  /** Short purpose text of an instance's profile ('' = none). */
  serverDescription(id: string): string {
    return this.profiles.find((p) => p.id === id)?.description ?? '';
  }

  /** Falls back to the default instance when the selected one no longer exists. */
  private ensureSelectionExists() {
    const id = this.selectedServerId;
    if (id === DEFAULT_SERVER_ID || this.servers.length === 0 || !this.profilesLoaded) return;
    const known = this.servers.some((s) => s.id === id) || this.profiles.some((p) => p.id === id);
    if (!known) this.selectServer(DEFAULT_SERVER_ID);
  }

  /** Replaces settings with a detached copy so no component shares a mutable proxy. */
  setSettings(next: AppSettings) {
    this.settings = structuredClone($state.snapshot(next) as AppSettings);
    // An unsaved Dashboard server takes its values from the settings defaults.
    if (this.defaultServer && !this.defaultServer.saved) void this.refreshDefaultServer();
  }

  /** Authorization header for requests to the running server. */
  authHeaders(): Record<string, string> {
    return this.apiKey ? { Authorization: `Bearer ${this.apiKey}` } : {};
  }
}

const SERVERS_CACHE_KEY = 'harness_servers_cache';

function loadCachedServers(): ServerSummary[] {
  try {
    const raw = typeof localStorage !== 'undefined' ? localStorage.getItem(SERVERS_CACHE_KEY) : null;
    return raw ? JSON.parse(raw) : [];
  } catch {
    return [];
  }
}

function loadSelectedServerId(): string {
  try {
    return localStorage.getItem(SELECTED_SERVER_KEY) || DEFAULT_SERVER_ID;
  } catch {
    return DEFAULT_SERVER_ID;
  }
}

function connectHostFor(bind: string | undefined): string {
  if (!bind || bind === '0.0.0.0') return '127.0.0.1';
  if (bind === '::' || bind === '[::]') return '[::1]';
  return bind;
}

export const app = new AppState();
