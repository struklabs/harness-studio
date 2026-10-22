<script lang="ts">
  import { invoke } from '@tauri-apps/api/core';
  import { onMount } from 'svelte';
  import { t, type TKey } from '#lib/i18n.svelte.js';
  import { errorText } from '#lib/errors.js';
  import { Button } from '#lib/components/ui/button/index.js';
  import { Input } from '#lib/components/ui/input/index.js';
  import { app } from '#lib/stores/app.svelte.js';
  import { security } from '#lib/stores/security.svelte.js';
  import type { RegistryEntry } from '../tools/registryCatalog.ts';
  import OfficialSuggestions from '../tools/OfficialSuggestions.svelte';
  import { officialMatches, mcpConfigTemplates, mcpEvaluations, riskGroups, needsRiskAck, type GithubRepo, type GithubReview, type SkillReview } from './githubDiscovery.ts';
  let query = $state('');
  let token = $state('');
  let busy = $state(false);
  let message = $state('');
  let results = $state<GithubRepo[]>([]);
  let review = $state<GithubReview | null>(null);
  let selected = $state<SkillReview | null>(null);
  let confirmed = $state(false);
  let riskAck = $state(false);
  let catalog = $state<RegistryEntry[]>([]);
  let catalogError = $state(false);
  onMount(() => { void invoke<{ index: { entries: RegistryEntry[] } }>('official_registry_list').then(s => catalog = s.index.entries).catch(() => catalogError = true); });
  async function run(action: () => Promise<void>) {
    busy = true; message = '';
    try { await action(); } catch (e) { message = t('github.failed', { detail: errorText(e) }); } finally { busy = false; }
  }
  function search() { void run(async () => { review = null; selected = null; results = []; results = await invoke<GithubRepo[]>('github_discovery_search', { query }); if (!results.length) message = t('github.no_results'); }); }
  function inspect(repo: string) { void run(async () => { review = null; selected = null; confirmed = false; riskAck = false; review = await invoke<GithubReview>('github_discovery_inspect', { repo }); }); }
  let evaluations = $derived(review ? mcpEvaluations(review.mcp) : []);
  function install() {
    const r = review, s = selected;
    if (!r || !s || !confirmed || (needsRiskAck(s.security) && !riskAck)) return;
    void run(async () => {
      await invoke('github_discovery_install', { repo: r.repo, sha: r.sha, path: s.path, scripts: s.scripts, securityLevel: s.security.level, confirmed });
      selected = null; confirmed = false; riskAck = false; message = t('github.installed');
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
  {#if !results.length && query.trim()}<OfficialSuggestions kind="skill" query={query} />{/if}
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
      <details class="rounded border p-2">
        <summary class="text-sm font-medium">{t('github.transport_title')}</summary>
        <p class="mt-2 text-xs"><strong>{t('github.kind_local_stdio')}</strong> {t('github.stdio_explain')}</p>
        <p class="mt-2 text-xs"><strong>{t('github.kind_remote_http')}</strong> {t('github.remote_explain')}</p>
      </details>
      <h4 class="font-semibold">{t('github.mcp_eval_title')}</h4>
      {#each evaluations as view, i (i)}
        <div class="space-y-1 rounded border p-3">
          <p class="text-sm font-medium">{t(view.source === 'registry' ? 'github.source_registry' : 'github.source_manifest')}</p>
          <ul class="space-y-1 text-xs">
            {#each view.evaluation.checks as check (check.id)}
              <li>{t(`github.status_${check.status}` as TKey)} · {t(`github.check_${check.id}` as TKey)}{check.detail ? ` (${check.detail})` : ''}</li>
            {/each}
          </ul>
          {#each view.evaluation.entries as entry, j (j)}
            <p class="pt-1 text-sm font-medium">{t(`github.kind_${entry.kind}` as TKey)}{entry.label ? ` · ${entry.label}` : ''}</p>
            <ul class="space-y-1 text-xs">
              {#each entry.checks as check (check.id)}
                <li>{t(`github.status_${check.status}` as TKey)} · {t(`github.check_${check.id}` as TKey)}{check.detail ? ` (${check.detail})` : ''}</li>
              {/each}
            </ul>
          {/each}
        </div>
      {:else}
        <p class="text-xs text-muted-foreground">{t('github.mcp_eval_none')}</p>
      {/each}
      <textarea readonly class="h-52 w-full rounded border bg-background p-2 font-mono text-xs" aria-label={t('github.mcp') } value={JSON.stringify(review.mcp, null, 2)}></textarea>
      <Button variant="outline" disabled={busy || !Object.keys(mcpConfigTemplates(review.mcp).mcpServers).length} onclick={() => void run(async () => { await navigator.clipboard.writeText(JSON.stringify(mcpConfigTemplates(review?.mcp), null, 2)); message = t('github.copied'); })}>{t('github.copy')}</Button>
      {#each review.skills as skill (skill.path)}
        <article class="space-y-2 rounded border p-3">
          <h4 class="font-semibold">{skill.name || skill.path}</h4>
          <p class="text-xs">{skill.path} · {skill.bytes} B</p>
          <p class="text-sm">{t('github.validation', { status: skill.errors.length ? t('github.invalid') : t('github.valid') })}</p>
          {#each skill.errors as error}<p class="text-sm">{errorText(error)}</p>{/each}
          <p class="text-xs">{t('github.scripts', { scripts: skill.scripts.join(', ') || t('github.none') })}</p>
          <p class="text-xs">{t('github.network', { files: skill.network.join(', ') || t('github.none') })}</p>
          <p class="text-xs">{t('github.references', { count: skill.references.length })}</p>
          {#each skill.warnings as warning}<p class="text-xs text-warning">{errorText(warning)}</p>{/each}
          <h5 class="text-sm font-semibold">{t('github.security_title')}</h5>
          <p class="text-sm">{t('github.security_level', { level: t(`github.level_${skill.security.level}` as TKey) })}</p>
          <p class="text-xs">{t('github.risk_counts', { network: skill.security.network, shell: skill.security.shell, eval: skill.security.eval, credentials: skill.security.credentials, binary: skill.security.binary })}</p>
          {#if skill.risks.length}
            <details class="text-xs">
              <summary>{t('github.risk_details')}</summary>
              {#each riskGroups(skill.risks) as group (group.category)}
                <p class="mt-2 font-medium">{t(`github.cat_${group.category}` as TKey)}</p>
                <ul class="break-all">
                  {#each group.findings as f}<li>{f.file}{f.line ? `:${f.line}` : ''} · {f.pattern}</li>{/each}
                </ul>
              {/each}
            </details>
          {/if}
          <p class="text-xs break-all">{t('github.pinned', { sha: skill.commit || review.sha })}</p>
          <p class="text-xs break-all">{t('github.scope', { scope: t('github.scope_user'), dir: skill.install_dir })}</p>
          <p class="text-xs text-muted-foreground">{t('github.security_hint')}</p>
          <Button variant="outline" disabled={busy || skill.errors.length > 0} onclick={() => { selected = skill; confirmed = false; riskAck = false; }}>{t('github.pin')}</Button>
        </article>
      {/each}
      {#if selected}
        <section class="space-y-3 rounded border p-4" aria-label={t('github.confirm_title')}>
          <h4 class="font-semibold">{t('github.confirm_title')}</h4>
          <p class="break-all text-sm">{selected.name} · {review.sha}</p>
          <p>{t('github.scripts', { scripts: selected.scripts.join(', ') || t('github.none') })}</p>
          <p class="text-sm">{t('github.security_level', { level: t(`github.level_${selected.security.level}` as TKey) })}</p>
          <p class="break-all text-sm">{t('github.scope', { scope: t('github.scope_user'), dir: selected.install_dir })}</p>
          <p class="text-sm">{t('github.confirm_hint')}</p>
          <label class="flex gap-2 text-sm"><input type="checkbox" bind:checked={confirmed} disabled={busy} />{t('github.confirm')}</label>
          {#if needsRiskAck(selected.security)}
            <label class="flex gap-2 text-sm"><input type="checkbox" bind:checked={riskAck} disabled={busy} />{t('github.risk_ack')}</label>
          {/if}
          <Button disabled={busy || !confirmed || (needsRiskAck(selected.security) && !riskAck)} onclick={install}>{t('github.install')}</Button>
          <Button variant="outline" disabled={busy} onclick={() => selected = null}>{t('github.cancel')}</Button>
        </section>
      {/if}
    </section>
  {/if}
</section>
