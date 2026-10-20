<script lang="ts">
  import { onMount } from 'svelte';
  import { invoke } from '#lib/ipc.js';
  import { errorText } from '#lib/errors.js';
  import { locale, t, type TKey } from '#lib/i18n.svelte.js';
  import { Badge } from '#lib/components/ui/badge/index.js';
  import { Button } from '#lib/components/ui/button/index.js';
  import { Textarea } from '#lib/components/ui/textarea/index.js';
  import SettingGroup from './SettingGroup.svelte';

  type Kind = 'mcp_server' | 'agent' | 'trainer';
  type Level = 'safe' | 'restricted' | 'dangerous';
  type Capability = 'file' | 'network' | 'shell' | 'service' | 'account' | 'device';
  interface Grant { id: string; capability: Capability; paths: string[]; granted_at_ms: number }
  interface Subject { kind: Kind; id: string; label: string; available: boolean; trust: Level | null; inherent: Level; grantable: Capability[]; grants: Grant[] }

  let subjects = $state<Subject[]>([]);
  let error = $state('');
  let pending = $state(false);
  // Add-grant draft per subject.
  let draftCap = $state<Record<string, Capability>>({});
  let draftPaths = $state<Record<string, string>>({});

  const key = (s: Subject) => s.kind + ':' + s.id;
  const capLabel = (c: Capability) => t(('perm.cap_' + c) as TKey);
  const levelLabel = (l: Level) => t(('perm.level_' + l) as TKey);
  const groups: { kind: Kind; title: TKey }[] = [
    { kind: 'mcp_server', title: 'perm.group_mcp' },
    { kind: 'agent', title: 'perm.group_agent' },
    { kind: 'trainer', title: 'perm.group_trainer' },
  ];

  async function load() {
    try { subjects = await invoke<Subject[]>('permissions_list'); }
    catch (e) { error = errorText(e); }
  }
  onMount(() => { void load(); });

  async function act(command: string, args: Record<string, unknown>) {
    pending = true; error = '';
    try { await invoke(command, args); await load(); }
    catch (e) { error = errorText(e); }
    finally { pending = false; }
  }
  const grant = (s: Subject) => {
    const capability = draftCap[key(s)] ?? s.grantable[0];
    const paths = capability === 'file' ? (draftPaths[key(s)] ?? '').split('\n').map(p => p.trim()).filter(Boolean) : [];
    void act('permissions_grant', { kind: s.kind, id: s.id, capability, paths }).then(() => { if (!error) draftPaths[key(s)] = ''; });
  };
  const revokeGrant = (s: Subject, g: Grant) => act('permissions_revoke_grant', { kind: s.kind, id: s.id, grantId: g.id });
  const revokeAll = (s: Subject) => act('permissions_revoke_subject', { kind: s.kind, id: s.id });
  const setTrust = (s: Subject, value: string) => act('permissions_set_trust', { kind: s.kind, id: s.id, trust: value === '' ? null : value });
  const since = (g: Grant) => t('perm.grant_since', { date: new Date(g.granted_at_ms).toLocaleString(locale()) });
</script>

<div class="flex flex-col gap-4" data-testid="permissions-manager">
  <div class="flex flex-col gap-1">
    <h3 class="text-sm font-semibold">{t('perm.title')}</h3>
    <p class="text-xs text-muted-foreground">{t('perm.intro')}</p>
    <p class="text-xs text-muted-foreground">{t('perm.effect')}</p>
  </div>
  {#if error}<p role="alert" class="text-sm text-destructive">{error}</p>{/if}

  {#each groups as group (group.kind)}
    {@const rows = subjects.filter(s => s.kind === group.kind)}
    <SettingGroup title={t(group.title)}>
      {#each rows as s (key(s))}
        <div class="flex flex-col gap-2 border-b px-4 py-3 last:border-b-0" data-subject={key(s)}>
          <div class="flex flex-wrap items-center gap-2">
            <span class="break-all text-sm font-medium">{s.label}</span>
            {#if s.kind === 'trainer' || (!s.available && s.kind === 'agent')}
              <Badge variant="outline">{t('perm.not_available')}</Badge>
            {:else if !s.available}
              <Badge variant="outline">{t('perm.removed')}</Badge>
            {/if}
            <span class="text-xs text-muted-foreground">{t('perm.inherent', { level: levelLabel(s.inherent) })}</span>
            <span class="flex-1"></span>
            {#if s.grants.length || s.trust}
              <Button variant="outline" size="sm" disabled={pending} onclick={() => revokeAll(s)}>{t('perm.revoke_all')}</Button>
            {/if}
          </div>

          {#if s.kind === 'trainer'}
            <p class="text-xs text-muted-foreground">{t('perm.trainer_note')}</p>
          {:else}
            <label class="flex flex-wrap items-center gap-2 text-xs">
              <span class="font-medium">{t('perm.trust')}</span>
              <select class="rounded border bg-background p-1.5 text-sm" disabled={pending} value={s.trust ?? ''} onchange={(e) => setTrust(s, e.currentTarget.value)}>
                <option value="">{t('perm.trust_default')}</option>
                {#each ['safe', 'restricted', 'dangerous'] as level (level)}<option value={level}>{levelLabel(level as Level)}</option>{/each}
              </select>
              <span class="text-muted-foreground">{t(s.kind === 'agent' ? 'perm.trust_help_agent' : 'perm.trust_help_mcp')}</span>
            </label>

            <div class="flex flex-col gap-1">
              <span class="text-xs font-medium">{t('perm.grants')}</span>
              {#each s.grants as g (g.id)}
                <div class="flex flex-wrap items-center gap-2 rounded border px-2 py-1 text-xs" data-grant={g.id}>
                  <Badge variant="secondary">{capLabel(g.capability)}</Badge>
                  {#if g.paths.length}<span class="break-all font-mono">{g.paths.join(' · ')}</span>{/if}
                  <span class="text-muted-foreground">{since(g)}</span>
                  <span class="flex-1"></span>
                  <Button variant="outline" size="sm" disabled={pending} onclick={() => revokeGrant(s, g)}>{t('perm.revoke')}</Button>
                </div>
              {:else}
                <p class="text-xs text-muted-foreground">{t('perm.no_grants')}</p>
              {/each}
            </div>

            <div class="flex flex-wrap items-end gap-2">
              <label class="flex flex-col gap-1 text-xs">
                <span>{t('perm.capability')}</span>
                <select class="rounded border bg-background p-1.5 text-sm" bind:value={draftCap[key(s)]}>
                  {#each s.grantable as c (c)}<option value={c}>{capLabel(c)}</option>{/each}
                </select>
              </label>
              {#if (draftCap[key(s)] ?? s.grantable[0]) === 'file'}
                <label class="flex min-w-48 flex-1 flex-col gap-1 text-xs">
                  <span>{t('perm.paths')}</span>
                  <Textarea rows={2} class="font-mono text-xs" spellcheck={false} bind:value={draftPaths[key(s)]} />
                </label>
              {/if}
              <Button size="sm" disabled={pending} onclick={() => grant(s)}>{t('perm.add')}</Button>
            </div>
            {#if s.kind === 'mcp_server'}<p class="text-xs text-muted-foreground">{t('perm.mcp_grant_note')}</p>{/if}
          {/if}
        </div>
      {:else}
        {#if group.kind === 'mcp_server'}<p class="px-4 py-3 text-xs text-muted-foreground">{t('perm.none_mcp')}</p>{/if}
      {/each}
    </SettingGroup>
  {/each}
</div>
