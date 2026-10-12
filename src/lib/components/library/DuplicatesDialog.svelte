<script lang="ts">
  import AdvancedOnly from '../common/AdvancedOnly.svelte';
  // Duplicate models: groups with the same size and first/last MiB, "Tam doğrula" compares the
  // full SHA-256 (recorded download hashes are reused). Extra copies are deleted through the
  // regular delete dialog (same safety rules).
  import { invoke } from '$lib/ipc';
  import { onMount } from 'svelte';
  import type { DuplicateGroup, VerifiedMember } from '$lib/types';
  import { t } from '$lib/i18n.svelte';
  import { formatBytes } from '$lib/format';
  import * as Dialog from '$lib/components/ui/dialog';
  import { Button } from '$lib/components/ui/button';
  import { Skeleton } from '$lib/components/ui/skeleton';
  import { CircleCheck, CircleAlert, LoaderCircle, ShieldCheck, Trash2, Unlink } from '@lucide/svelte';

  interface Props {
    /** Bumped by the parent after a delete so the groups are searched again. */
    version: number;
    onDelete: (modelId: string) => void;
    onClose: () => void;
  }

  let { version, onDelete, onClose }: Props = $props();

  let groups = $state<DuplicateGroup[] | null>(null);
  let error = $state<string | null>(null);
  let verified = $state<Record<string, VerifiedMember[]>>({});
  let verifying = $state<Record<string, boolean>>({});

  async function search() {
    error = null;
    try {
      groups = await invoke<DuplicateGroup[]>('find_duplicate_models');
    } catch (e) {
      error = String(e);
      groups = [];
    }
  }

  onMount(search);
  let lastVersion: number | undefined;
  $effect(() => {
    const v = version;
    if (lastVersion !== undefined && v !== lastVersion) {
      verified = {};
      void search();
    }
    lastVersion = v;
  });

  async function verify(g: DuplicateGroup) {
    verifying[g.pre_hash] = true;
    try {
      verified[g.pre_hash] = await invoke<VerifiedMember[]>('verify_duplicate_models', {
        modelIds: g.members.map((m) => m.model_id),
      });
    } catch (e) {
      error = String(e);
    } finally {
      verifying[g.pre_hash] = false;
    }
  }

  function hashOf(g: DuplicateGroup, id: string): VerifiedMember | undefined {
    return verified[g.pre_hash]?.find((v) => v.model_id === id);
  }
  /** Members whose full hash equals another member's. */
  function isCopy(g: DuplicateGroup, id: string): boolean | null {
    const list = verified[g.pre_hash];
    const me = hashOf(g, id);
    if (!list || !me) return null;
    return list.some((v) => v.model_id !== id && v.sha256 === me.sha256);
  }
</script>

<Dialog.Root open={true} onOpenChange={(open) => !open && onClose()}>
  <Dialog.Content class="flex max-h-[85vh] flex-col sm:max-w-2xl">
    <Dialog.Header>
      <Dialog.Title>{t('lib.dupes_title')}</Dialog.Title>
      <Dialog.Description>{t('lib.dupes_desc')}</Dialog.Description>
    </Dialog.Header>

    <div class="flex min-h-0 flex-1 flex-col gap-3 overflow-y-auto">
      {#if groups === null}
        <div class="flex flex-col gap-2" role="status" aria-label={t('lib.dupes_searching')}>
          <Skeleton class="h-16 rounded-md" /><Skeleton class="h-16 rounded-md" />
        </div>
      {:else if groups.length === 0}
        <p class="py-6 text-center text-xs text-muted-foreground">{t('lib.dupes_none')}</p>
      {:else}
        {#each groups as g (g.pre_hash)}
          <section class="flex flex-col gap-1.5 rounded-md border border-border p-2.5">
            <div class="flex items-center justify-between gap-2 text-xs">
              <span class="text-muted-foreground">{t('lib.dupes_group', { count: g.members.length, size: formatBytes(g.size_bytes) })}</span>
              <Button size="xs" variant="outline" onclick={() => verify(g)} disabled={verifying[g.pre_hash]}>
                {#if verifying[g.pre_hash]}<LoaderCircle class="animate-spin" />{t('lib.dupes_verifying')}{:else}<ShieldCheck />{t('lib.dupes_verify')}{/if}
              </Button>
            </div>
            <ul class="flex flex-col gap-1">
              {#each g.members as mem (mem.model_id)}
                {@const copy = isCopy(g, mem.model_id)}
                {@const h = hashOf(g, mem.model_id)}
                <li class="flex min-w-0 items-center gap-2 text-xs">
                  {#if copy === true}
                    <CircleCheck class="size-3.5 shrink-0 text-success" aria-label={t('lib.dupes_same')} />
                  {:else if copy === false}
                    <CircleAlert class="size-3.5 shrink-0 text-warning" aria-label={t('lib.dupes_differs')} />
                  {/if}
                  <div class="flex min-w-0 flex-1 flex-col">
                    <span class="truncate">{mem.name}{mem.origin === 'linked' ? ` · ${t('lib.linked_short')}` : ''}</span>
                    <span class="truncate font-mono text-xs text-muted-foreground" title={mem.path}>{mem.path}</span>
                    <AdvancedOnly>{#if h}
                      <span class="truncate font-mono text-xs text-muted-foreground" title={h.sha256}>
                        sha256 {h.sha256.slice(0, 16)}…{h.recorded ? ` · ${t('lib.dupes_recorded')}` : ''}
                      </span>
                    {/if}</AdvancedOnly>
                  </div>
                  <Button size="xs" variant="ghost" onclick={() => onDelete(mem.model_id)}>
                    {#if mem.origin === 'linked'}<Unlink />{t('lib.unlink')}{:else}<Trash2 />{t('lib.delete')}{/if}
                  </Button>
                </li>
              {/each}
            </ul>
            {#if verified[g.pre_hash] && g.members.every((m) => isCopy(g, m.model_id) === false)}
              <p class="text-xs text-warning">{t('lib.dupes_not_identical')}</p>
            {/if}
          </section>
        {/each}
      {/if}
      {#if error}
        <p class="rounded-md border border-destructive/30 bg-destructive/10 px-2.5 py-2 text-xs text-destructive" role="alert">{error}</p>
      {/if}
    </div>
    <Dialog.Footer>
      <Button variant="outline" onclick={onClose}>{t('def.close')}</Button>
    </Dialog.Footer>
  </Dialog.Content>
</Dialog.Root>
