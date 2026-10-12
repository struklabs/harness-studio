<script lang="ts">
  import FieldHelp from '$lib/components/common/FieldHelp.svelte';
  // "Unsaved changes vs definition" indicator of the Dashboard form with the explicit save
  // actions. Starting a server never saves; these buttons do.
  import type { ModelDefinition } from '$lib/types';
  import { t } from '$lib/i18n.svelte';
  import { domain } from '$lib/stores/domain.svelte';
  import { shortWindow } from '$lib/stores/panes.svelte';
  import { Button, buttonVariants } from '$lib/components/ui/button';
  import { Input } from '$lib/components/ui/input';
  import * as DropdownMenu from '$lib/components/ui/dropdown-menu';
  import { cn } from '$lib/utils';
  import { ChevronDown, CircleDot, Undo2 } from '@lucide/svelte';

  interface Props {
    definition: ModelDefinition | null;
    /** Alias suggestion when the file has no definition yet (file stem). */
    fileAlias: string;
    /** Number of settings that differ from the definition (0 = in sync). */
    changes: number;
    /** Profiles that use the definition (saving changes them too). */
    usedBy: string[];
    busy: boolean;
    onSaveDefault: () => void;
    onSaveNew: (alias: string) => void;
    onRevert: () => void;
  }

  let { definition, fileAlias, changes, usedBy, busy, onSaveDefault, onSaveNew, onRevert }: Props = $props();

  const uid = $props.id();
  let naming = $state(false);
  let alias = $state('');

  const ALIAS_RE = /^[A-Za-z0-9._:-]{1,128}$/;
  let aliasError = $derived.by(() => {
    const a = alias.trim();
    if (!ALIAS_RE.test(a)) return t('def.alias_invalid');
    const clash = domain.definitions.find((d) => d.alias.toLowerCase() === a.toLowerCase());
    return clash ? t('def.alias_taken', { alias: clash.alias }) : null;
  });

  function suggest(base: string): string {
    const clean = base.replace(/[^A-Za-z0-9._:-]+/g, '-').replace(/^-+|-+$/g, '') || 'model';
    const taken = new Set(domain.definitions.map((d) => d.alias.toLowerCase()));
    if (!taken.has(clean.toLowerCase())) return clean;
    for (let n = 2; ; n++) if (!taken.has(`${clean}-${n}`.toLowerCase())) return `${clean}-${n}`;
  }

  function startNaming() {
    alias = suggest(definition ? definition.alias : fileAlias);
    naming = true;
  }

  function submit(e: SubmitEvent) {
    e.preventDefault();
    if (aliasError) return;
    naming = false;
    onSaveNew(alias.trim());
  }
</script>

<!-- Short window: one line (status + "Save" menu); the full text and affected profiles are in
     the menu, so the Dashboard's settings keep most of the column height. -->
{#snippet compactRow()}
  {@const long = !definition ? t('def.no_definition_hint') : t('def.unsaved', { count: changes, alias: definition.alias })}
  <div class="flex min-w-0 items-center gap-2 text-xs">
    {#if definition}<CircleDot class="size-3 shrink-0 text-warning" aria-hidden="true" />{/if}
    <span class={cn('min-w-0 flex-1 truncate', definition ? 'text-warning' : 'text-muted-foreground')} role="status" title={long}>
      {definition ? t('def.unsaved_short', { count: changes }) : t('def.no_definition_short')}
    </span>
    <DropdownMenu.Root>
      <DropdownMenu.Trigger class={cn(buttonVariants({ variant: 'outline', size: 'xs' }), 'shrink-0')} disabled={busy}>
        {t('def.save_menu')}
        <ChevronDown />
      </DropdownMenu.Trigger>
      <DropdownMenu.Content align="end" class="w-64">
        <p class="px-2 py-1.5 text-xs leading-snug text-muted-foreground">{long}</p>
        {#if definition && usedBy.length}
          <p class="px-2 pb-1.5 text-xs leading-snug text-muted-foreground">{t('def.save_default_profiles', { names: usedBy.join(', ') })}</p>
        {/if}
        <DropdownMenu.Separator />
        {#if definition}
          <DropdownMenu.Item onclick={onSaveDefault}>{t('def.save_default')}</DropdownMenu.Item>
        {/if}
        <DropdownMenu.Item onclick={startNaming}>{definition ? t('def.save_new') : t('def.save_as_definition')}</DropdownMenu.Item>
        {#if definition}
          <DropdownMenu.Separator />
          <DropdownMenu.Item onclick={onRevert} title={t('def.revert_title')}>
            <Undo2 />{t('def.revert')}
          </DropdownMenu.Item>
        {/if}
      </DropdownMenu.Content>
    </DropdownMenu.Root>
  </div>
{/snippet}

<div class="flex flex-col gap-1.5 rounded-md border border-border bg-muted/30 px-2.5 py-2 short:py-1.5" role="group" aria-label={t('def.save_bar_label')}>
  {#if shortWindow.current && !naming && (!definition || changes > 0)}
    {@render compactRow()}
  {:else}
    <div class="flex min-w-0 items-center gap-2 text-xs">
      {#if !definition}
        <span class="min-w-0 flex-1 text-muted-foreground">{t('def.no_definition_hint')}</span>
      {:else if changes > 0}
        <CircleDot class="size-3 shrink-0 text-warning" aria-hidden="true" />
        <span class="min-w-0 flex-1 text-warning" role="status">{t('def.unsaved', { count: changes, alias: definition.alias })}</span>
        <Button variant="ghost" size="xs" class="h-5 px-1.5 text-xs" onclick={onRevert} disabled={busy} title={t('def.revert_title')}>
          <Undo2 />{t('def.revert')}
        </Button>
      {:else}
        <span class="min-w-0 flex-1 truncate text-muted-foreground" role="status">{t('def.in_sync', { alias: definition.alias })}</span>
      {/if}
    </div>
    {#if naming}
      <form class="flex items-start gap-1.5" onsubmit={submit}>
        <div class="flex min-w-0 flex-1 flex-col gap-0.5">
          <FieldHelp text={t('help.alias')}><Input id="{uid}-alias" class="h-7 font-mono text-xs" bind:value={alias} aria-label={t('def.alias')} aria-invalid={aliasError ? 'true' : undefined} /></FieldHelp>
          {#if aliasError}<span class="text-xs text-destructive">{aliasError}</span>{/if}
        </div>
        <Button type="submit" variant="outline" size="xs" disabled={busy || !!aliasError}>{t('ip.save')}</Button>
        <Button type="button" size="xs" variant="ghost" onclick={() => (naming = false)}>{t('ip.cancel')}</Button>
      </form>
    {:else if !definition || changes > 0}
      <div class="flex flex-wrap gap-1.5">
        {#if definition}
          <Button size="xs" variant="outline" onclick={onSaveDefault} disabled={busy} title={usedBy.length ? t('def.save_default_profiles', { names: usedBy.join(', ') }) : t('def.save_default_title')}>
            {t('def.save_default')}
          </Button>
        {/if}
        <Button size="xs" variant="ghost" onclick={startNaming} disabled={busy}>
          {definition ? t('def.save_new') : t('def.save_as_definition')}
        </Button>
      </div>
      {#if definition && usedBy.length}
        <p class="text-xs leading-snug text-muted-foreground">{t('def.save_default_profiles', { names: usedBy.join(', ') })}</p>
      {/if}
    {/if}
  {/if}
</div>
