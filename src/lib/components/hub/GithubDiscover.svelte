<script lang="ts">
  import { invoke } from '@tauri-apps/api/core';
  import { onMount } from 'svelte';
  import { t } from '#lib/i18n.svelte.js';
  import { Button } from '#lib/components/ui/button/index.js';
  import { Input } from '#lib/components/ui/input/index.js';
  import { app } from '#lib/stores/app.svelte.js';
  import { security } from '#lib/stores/security.svelte.js';
  import type { RegistryEntry } from '../tools/registryCatalog.ts';
  import { officialMatches, mcpConfigTemplates, type GithubRepo, type GithubReview, type SkillReview } from './githubDiscovery.ts';
  let query = $state('');
  let token = $state('');
  let busy = $state(false);
  let message = $state('');
  let results = $state<GithubRepo[]>([]);
  let review = $state<GithubReview | null>(null);
  let selected = $state<SkillReview | null>(null);
  let confirmed = $state(false);
  let catalog = $state<RegistryEntry[]>([]);
  let catalogError = $state(false);
  onMount(() => { void invoke<{ index: { entries: RegistryEntry[] } }>('official_registry_list').then(s => catalog = s.index.entries).catch(() => catalogError = true); });
  async function run(action: () => Promise<void>) {
    busy = true; message = '';
    try { await action(); } catch (e) { message = t('github.failed', { detail: String(e) }); } finally { busy = false; }
  }
  function search() { void run(async () => { review = null; selected = null; results = []; results = await invoke<GithubRepo[]>('github_discovery_search', { query }); if (!results.length) message = t('github.no_results'); }); }
  function inspect(repo: string) { void run(async () => { review = null; selected = null; confirmed = false; review = await invoke<GithubReview>('github_discovery_inspect', { repo }); }); }
  function install() {
    const r = review, s = selected;
    if (!r || !s || !confirmed) return;
    void run(async () => {
      await invoke('github_discovery_install', { repo: r.repo, sha: r.sha, path: s.path, scripts: s.scripts, confirmed });
      selected = null; confirmed = false; message = t('github.installed');
    });
  }
</script>

<section class="flex min-w-0 flex-col gap-4" aria-label={t('github.tab')}>
  <p class="text-sm text-muted-foreground">{t('github.intro')}</p>
  <form class="flex gap-2" onsubmit={(e) => { e.preventDefault(); search(); }}>
    <Input aria-label={t('github.query')} placeholder={t('github.query')} bind:value={query} />
    <Button type="submit" disabled={busy || !query.trim()}>{t('github.search')}</Button>
  </form>
  <details class="rounded-lg border p-3">
    <summary>{t('github.token')}</summary>
    <p class="my-2 text-xs text-muted-foreground">{t('github.token_hint')}</p>
    <form class="flex gap-2" onsubmit={(e) => { e.preventDefault(); void run(async () => { await invoke('github_discovery_token', { token }); token = ''; message = t('github.token_saved'); }); }}>
      <Input type="password" autocomplete="off" aria-label={t('github.token')} bind:value={token} />
      <Button type="submit" disabled={busy}>{t('github.save_token')}</Button>
    </form>
  </details>
  {#if busy}<p role="status">{t('github.loading')}</p>{/if}
  {#if message}<p role="status" class="text-sm break-words">{message}</p>{/if}
  {#if catalogError}<p class="text-xs">{t('github.catalog_unavailable')}</p>{/if}
  {#each results as repo (repo.full_name)}
    <article class="space-y-2 rounded-lg border p-4">
      <h3 class="font-semibold">{repo.full_name}</h3>
      <p class="text-xs font-medium">{t('github.unofficial')}</p>
      <p class="text-sm">{repo.description ?? ''}</p>
      <p class="text-xs">{t('github.stats', { stars: repo.stargazers_count, date: repo.updated_at, license: repo.license?.spdx_id ?? t('github.unknown') })}</p>
      <p class="text-xs">{repo.topics?.join(' · ')}</p>
      {#each officialMatches(repo.full_name, catalog) as entry (entry.id)}
        <p class="text-sm">{t('github.official_hint', { name: entry.name })}</p>
        <Button variant="outline" onclick={() => { app.requestedRegistryKind = entry.type; security.openSettings('tools-installed'); }}>{t('github.open_catalog')}</Button>
      {/each}
      <Button disabled={busy} variant="outline" onclick={() => inspect(repo.full_name)}>{t('github.review')}</Button>
    </article>
  {/each}
  {#if review}
    <section class="space-y-3 rounded-lg border p-4" aria-label={t('github.review')}>
      <h3 class="font-semibold">{review.repo}</h3>
      <p>{t('github.unofficial')}</p>
      <p class="break-all text-xs">{t('github.commit', { sha: review.sha })}</p>
      <p class="text-sm">{t('github.release', { version: review.release?.tag_name ?? t('github.unknown') })}</p>
      <p class="text-sm">{t('github.platforms', { platforms: review.platforms.join(', ') || t('github.unknown') })}</p>
      <h4 class="font-semibold">{t('github.readme')}</h4>
      <pre class="max-h-64 overflow-auto whitespace-pre-wrap break-words text-xs">{review.readme || t('github.unknown')}</pre>
      <h4 class="font-semibold">{t('github.mcp')}</h4>
      <p class="text-xs text-muted-foreground">{t('github.mcp_hint')}</p>
      <textarea readonly class="h-52 w-full rounded border bg-background p-2 font-mono text-xs" aria-label={t('github.mcp') } value={JSON.stringify(review.mcp, null, 2)}></textarea>
      <Button variant="outline" disabled={busy || !Object.keys(mcpConfigTemplates(review.mcp).mcpServers).length} onclick={() => void run(async () => { await navigator.clipboard.writeText(JSON.stringify(mcpConfigTemplates(review?.mcp), null, 2)); message = t('github.copied'); })}>{t('github.copy')}</Button>
      {#each review.skills as skill (skill.path)}
        <article class="space-y-2 rounded border p-3">
          <h4 class="font-semibold">{skill.name || skill.path}</h4>
          <p class="text-xs">{skill.path} · {skill.bytes} B</p>
          <p class="text-sm">{t('github.validation', { status: skill.errors.length ? t('github.invalid') : t('github.valid') })}</p>
          {#each skill.errors as error}<p class="text-sm">{error}</p>{/each}
          <p class="text-xs">{t('github.scripts', { scripts: skill.scripts.join(', ') || t('github.none') })}</p>
          <p class="text-xs">{t('github.network', { files: skill.network.join(', ') || t('github.none') })}</p>
          <p class="text-xs text-muted-foreground">{t('github.security_hint')}</p>
          <Button variant="outline" disabled={busy || skill.errors.length > 0} onclick={() => { selected = skill; confirmed = false; }}>{t('github.pin')}</Button>
        </article>
      {/each}
      {#if selected}
        <section class="space-y-3 rounded border p-4" aria-label={t('github.confirm_title')}>
          <h4 class="font-semibold">{t('github.confirm_title')}</h4>
          <p class="break-all text-sm">{selected.name} · {review.sha}</p>
          <p>{t('github.scripts', { scripts: selected.scripts.join(', ') || t('github.none') })}</p>
          <p class="text-sm">{t('github.confirm_hint')}</p>
          <label class="flex gap-2 text-sm"><input type="checkbox" bind:checked={confirmed} disabled={busy} />{t('github.confirm')}</label>
          <Button disabled={busy || !confirmed} onclick={install}>{t('github.install')}</Button>
          <Button variant="outline" disabled={busy} onclick={() => selected = null}>{t('github.cancel')}</Button>
        </section>
      {/if}
    </section>
  {/if}
</section>
