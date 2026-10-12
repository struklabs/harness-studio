import type { ServerStatus, ServerSummary } from '$lib/types';
import type { TKey } from '$lib/i18n.svelte';
import { localizeError } from '$lib/network';

export type StatusKind = 'running' | 'starting' | 'stopping' | 'stopped' | 'error';

export function statusKind(status: ServerStatus | undefined): StatusKind {
  if (!status) return 'stopped';
  if (typeof status === 'object') return 'error';
  switch (status) {
    case 'Running':
      return 'running';
    case 'Starting':
      return 'starting';
    case 'Stopping':
      return 'stopping';
    default:
      return 'stopped';
  }
}

export const STATUS_LABEL: Record<StatusKind, TKey> = {
  running: 'srv.status_running',
  starting: 'srv.status_starting',
  stopping: 'srv.status_stopping',
  stopped: 'srv.status_stopped',
  error: 'srv.status_error',
};

export function statusError(status: ServerStatus | undefined): string | null {
  return status && typeof status === 'object' && 'Error' in status ? localizeError(status.Error) : null;
}

/** Starting, running or stopping: the process exists and holds its port. */
export function isLive(status: ServerStatus | undefined): boolean {
  const k = statusKind(status);
  return k === 'running' || k === 'starting' || k === 'stopping';
}

/** Last path component (works for both / and \ separators). */
export function fileName(path: string | null | undefined): string {
  if (!path) return '';
  const parts = path.split(/[\\/]/).filter(Boolean);
  return parts[parts.length - 1] ?? path;
}

const ID_RE = /^[A-Za-z0-9_-]{1,64}$/;

/** Profile id from a display name: ascii slug + short random suffix, e.g. "qwen-coder-4k2f". */
export function makeProfileId(name: string, taken: Iterable<string> = []): string {
  const used = new Set(taken);
  const slug =
    name
      .normalize('NFKD')
      .replace(/[̀-ͯ]/g, '')
      .replace(/ı/g, 'i')
      .toLowerCase()
      .replace(/[^a-z0-9]+/g, '-')
      .replace(/^-+|-+$/g, '')
      .slice(0, 48) || 'profile';
  for (;;) {
    const id = `${slug}-${Math.random().toString(36).slice(2, 6)}`;
    if (id !== 'default' && ID_RE.test(id) && !used.has(id)) return id;
  }
}

/**
 * Another live instance of ours (not `selfId`) that already listens on `port`, if any.
 */
export function portOwner(servers: ServerSummary[], port: number, selfId: string): ServerSummary | null {
  return servers.find((s) => s.id !== selfId && isLive(s.status) && s.active?.port === port) ?? null;
}

/** Harness picks this launch's port at start (`port_auto`, or port 0). */
export function isAutoPort(cfg: { port: number; port_auto?: boolean }): boolean {
  return !!cfg.port_auto || !cfg.port;
}

/**
 * The port of a saved (not running) launch as text: the user's own, else "Automatic" (with
 * the last automatic pick when there is one).
 */
export function savedPortText(cfg: { port: number; port_auto?: boolean }, autoLabel: string): string {
  if (!isAutoPort(cfg)) return `:${cfg.port}`;
  return cfg.port ? `${autoLabel} · :${cfg.port}` : autoLabel;
}
