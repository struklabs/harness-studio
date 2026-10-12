import { invoke } from '#lib/ipc.js';
import type { ReleaseAsset, ReleaseAssetInfo } from '#lib/types.js';
import { INTEGRITY_UNVERIFIED, unverifiedAssets } from '#lib/network.js';
import { rawError } from '#lib/errors.js';
import { app } from './app.svelte';

/** A start the backend refused because a network bind has no API key (`network-exposure-blocked`). */
export interface ExposureBlocked {
  serverId: string;
  host: string;
}

interface IntegrityPrompt {
  tag: string;
  assets: ReleaseAsset[];
  resolve: (confirmed: boolean) => void;
}

/** State of the app-wide security dialogs (mounted once in SecurityDialogs.svelte). */
class SecurityState {
  blocked = $state<ExposureBlocked | null>(null);
  integrity = $state<IntegrityPrompt | null>(null);
  /** Settings section another component wants opened (consumed by Settings.svelte). */
  requestedSection = $state<string | null>(null);
  requestedSettingsSearch = $state<{ section: string; query: string } | null>(null);

  openSettings(section = 'security', query = '') {
    this.requestedSettingsSearch = query ? { section, query } : null;
    this.requestedSection = section;
    app.requestedTab = 'settings';
  }

  /** Asks before installing archives without a SHA-256 digest (resolves true right away when all are verified). */
  confirmUnverified(info: ReleaseAssetInfo): Promise<boolean> {
    const assets = unverifiedAssets(info);
    if (!assets.length) return Promise.resolve(true);
    this.integrity?.resolve(false);
    return new Promise((resolve) => {
      this.integrity = { tag: info.tag_name, assets, resolve };
    });
  }

  settleIntegrity(confirmed: boolean) {
    const prompt = this.integrity;
    this.integrity = null;
    prompt?.resolve(confirmed);
  }
}

export const security = new SecurityState();

/**
 * Downloads and stage-1-verifies a runtime release. Archives without a digest are only installed
 * after the user confirms (asset names, sizes and URLs are shown). Returns the verified version,
 * or null when the user declined.
 */
export async function downloadRelease(info: ReleaseAssetInfo): Promise<string | null> {
  let allowUnverified = false;
  if (unverifiedAssets(info).length) {
    if (!(await security.confirmUnverified(info))) return null;
    allowUnverified = true;
  }
  const run = () =>
    invoke<string>('download_and_verify_staging', {
      downloadUrl: info.download_url,
      extraUrls: info.extra_assets.map((a) => a.download_url),
      allowUnverified,
    });
  try {
    return await run();
  } catch (e) {
    // The digest can disappear between the check and the download (release edited meanwhile).
    if (rawError(e) !== INTEGRITY_UNVERIFIED || allowUnverified) throw e;
    const all: ReleaseAssetInfo = { ...info, integrity: 'missing' };
    if (!(await security.confirmUnverified(all))) return null;
    allowUnverified = true;
    return await run();
  }
}
