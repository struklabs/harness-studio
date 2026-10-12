import type { BindScope, ReleaseAsset, ReleaseAssetInfo } from '$lib/types';
import { type TKey } from '$lib/i18n.svelte';
import { errorText } from '$lib/errors';

/** Error keys shared with the CLI's English table (harness-cli `messages.rs` checks the texts match).
 * The GUI translates every backend key through `$lib/errors`, not only these. */
export const ERROR_KEYS = [
  'network.lan_without_key',
  'network.tools_without_key',
  'network.invalid_host',
  'network.invalid_origin',
  'network.key_generation_failed',
  'runtime.integrity_unverified',
  'api.network_not_allowed',
  'api.network_without_token',
  'api.invalid_port',
  'api.invalid_origin',
  'api.token_name_invalid',
  'api.token_scopes_empty',
  'api.token_not_found',
  'api.random_failed',
  'api.bind_failed',
  'api.not_owner',
  'models.in_use',
  'models.not_found',
  'benchmark.already_running',
  'downloads.variant_not_found',
  'downloads.variant_ambiguous',
  'definitions.alias_not_found',
  'servers.already_active',
  'servers.definition_required',
  'servers.profile_definition_mismatch',
  'runtime.backend_no_device',
  'runtime.not_found',
  'runtime.id_taken',
  'runtime.invalid_id',
  'runtime.no_server_binary',
  'runtime.not_runnable',
  'runtime.changed',
  'runtime.in_use',
  'runtime.official_fixed',
  'runtime.quant_unsupported',
  'runtime.bad_repo',
  'runtime.no_release',
  'runtime.whisper_no_macos',
  'runtime.github_error',
  'servers.port_in_use',
  'servers.alias_in_use',
  'servers.no_failure',
  'gateway.invalid_port',
  'gateway.network_not_allowed',
  'gateway.network_without_token',
  'gateway.invalid_origin',
  'gateway.port_conflict',
  'gateway.not_owner',
  'gateway.bind_failed',
  'gateway.ollama_port_conflict',
  'gateway.ollama_port_ollama',
  'gateway.ollama_port_harness',
  'gateway.ollama_port_busy',
  'gateway.ollama_port_installed',
] as const satisfies readonly TKey[];

export const LAN_WITHOUT_KEY = 'network.lan_without_key';
export const INTEGRITY_UNVERIFIED = 'runtime.integrity_unverified';

/** Localized text of a backend error (see `$lib/errors`). */
export function localizeError(e: unknown): string {
  return errorText(e);
}

function parseIpv4(s: string): number[] | null {
  const parts = s.split('.');
  if (parts.length !== 4) return null;
  const nums = parts.map((p) => (/^\d{1,3}$/.test(p) ? Number(p) : NaN));
  return nums.every((n) => n >= 0 && n <= 255) ? nums : null;
}

function isIpv6(s: string): boolean {
  return /^[0-9a-f:.]+$/i.test(s) && s.includes(':') && (s.match(/::/g)?.length ?? 0) <= 1;
}

/** Mirrors harness_core::network::normalize_bind_host for form hints (the backend decides). */
export function isValidBindHost(host: string): boolean {
  const h = host.trim();
  if (h.toLowerCase() === 'localhost') return true;
  const bare = h.startsWith('[') && h.endsWith(']') ? h.slice(1, -1) : h;
  return parseIpv4(bare) !== null || isIpv6(bare);
}

export function bindScope(host: string | null | undefined): BindScope {
  const h = (host ?? '').trim().toLowerCase();
  const bare = h.startsWith('[') && h.endsWith(']') ? h.slice(1, -1) : h;
  if (bare === 'localhost' || bare === '::1') return 'loopback';
  const v4 = parseIpv4(bare.replace(/^::ffff:/, ''));
  return v4 && v4[0] === 127 ? 'loopback' : 'lan';
}

/** Archives of a release without a checkable SHA-256 digest. */
export function unverifiedAssets(info: ReleaseAssetInfo): ReleaseAsset[] {
  const all: ReleaseAsset[] = [
    {
      name: info.asset_name,
      download_url: info.download_url,
      size: info.size,
      digest: info.digest,
      integrity: info.integrity,
    },
    ...info.extra_assets,
  ];
  return all.filter((a) => (a.integrity ?? 'missing') !== 'verified');
}
