import type { ServerFailure, ServerStatus, ServerSummary } from '$lib/types';
import { t, type TKey } from '$lib/i18n.svelte';
import { localizeError } from '$lib/network';

/** Localized message of a classified start failure (`failure.<kind>` with its params). */
export function failureText(f: ServerFailure): string {
  return t(f.message_key as TKey, f.params);
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
  return `${(bytes / 1024 ** 3).toFixed(1)} GiB`;
}
