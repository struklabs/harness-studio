// Finding a running Harness Studio from Node: the owner of a data directory (the app or
// `harness serve`) describes its Control API in `<data dir>/instance.json`, and the session
// token is in `<data dir>/api-token`.
import { readFileSync } from 'node:fs';
import { homedir } from 'node:os';
import { join } from 'node:path';
import { createHarnessClient, type HarnessClient, type HarnessClientOptions } from './index.js';

export * from './index.js';

/** The per-user data directory: `HARNESS_DATA_DIR`, else the app's folder for this system. */
export function defaultDataDir(): string {
  if (process.env.HARNESS_DATA_DIR) return process.env.HARNESS_DATA_DIR;
  const app = 'com.harnessstudio.app';
  switch (process.platform) {
    case 'win32':
      return join(process.env.APPDATA ?? join(homedir(), 'AppData', 'Roaming'), app);
    case 'darwin':
      return join(homedir(), 'Library', 'Application Support', app);
    default:
      return join(process.env.XDG_DATA_HOME || join(homedir(), '.local', 'share'), app);
  }
}

export interface Discovered {
  /** `http://<host>:<port>` of the running owner's Control API. */
  baseUrl: string;
  /** The session token (full access). */
  token: string;
  dataDir: string;
}

/** Reads where the running instance listens and its session token. Throws when none runs. */
export function discover(dataDir: string = defaultDataDir()): Discovered {
  let info: { api?: { host: string; port: number } | null; token_path?: string | null };
  try {
    info = JSON.parse(readFileSync(join(dataDir, 'instance.json'), 'utf8'));
  } catch {
    throw new Error(`No running Harness Studio in ${dataDir} (start the app or \`harness serve\`)`);
  }
  if (!info.api) throw new Error('The running Harness Studio has the Control API turned off');
  // A wildcard listener is reached on loopback.
  const host = ['0.0.0.0', '::', '[::]'].includes(info.api.host) ? '127.0.0.1' : info.api.host;
  const token = readFileSync(info.token_path ?? join(dataDir, 'api-token'), 'utf8').trim();
  return { baseUrl: `http://${host.includes(':') ? `[${host}]` : host}:${info.api.port}`, token, dataDir };
}

/** A client for the running instance of `dataDir` (see {@link discover}). */
export function connect(options: HarnessClientOptions & { dataDir?: string } = {}): HarnessClient {
  const { dataDir, ...rest } = options;
  const found = discover(dataDir);
  return createHarnessClient({ baseUrl: found.baseUrl, token: found.token, ...rest });
}
