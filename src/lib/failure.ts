import { fixed } from '#lib/format.js';
import type { ServerFailure, ServerStatus, ServerSummary } from '#lib/types.js';
import { t, type TKey } from '#lib/i18n.svelte.js';
import { localizeError } from '#lib/network.js';

/** Localized message of a classified start failure (`failure.<kind>` with its params). */
export function failureText(f: ServerFailure): string {
  // Failures recorded before the engine was named in them were llama.cpp's.
  return t(f.message_key as TKey, { engine: 'llama.cpp', ...f.params });
}

/** What went wrong with a server: its classified failure, else the raw status error. */
export function serverErrorText(s: Pick<ServerSummary, 'status' | 'failure'> | null | undefined): string | null {
  if (!s) return null;
  const status: ServerStatus = s.status;
  const raw = status && typeof status === 'object' && 'Error' in status ? status.Error : null;
  if (!raw) return null;
  return s.failure ? failureText(s.failure) : localizeError(raw);
}

/** Recovery options exist for out-of-memory failures. */
export function canRecover(f: ServerFailure | null | undefined): boolean {
  return f?.kind === 'out_of_memory';
}

/** Binary units for memory figures (`15.0 GiB`). */
export function gib(bytes: number | null | undefined): string {
  if (bytes === null || bytes === undefined) return '—';
  return `${fixed(bytes / 1024 ** 3, 1)} GiB`;
}
