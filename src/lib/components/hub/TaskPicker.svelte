<script lang="ts">
  import AdvancedOnly from '../common/AdvancedOnly.svelte';
  import FieldHelp from '#lib/components/common/FieldHelp.svelte';
  import { t, type TKey } from '#lib/i18n.svelte.js';
  import * as Popover from '#lib/components/ui/popover/index.js';
  import { cn } from '#lib/utils.js';
  import { ChevronDown, LayoutGrid } from '@lucide/svelte';
  import { QUICK_TASKS, TASKS, TASK_GROUPS, taskInfo, type HubTask, type TaskSupport } from './tasks';
  import { taskSupportStore } from './taskSupport.svelte';
  import { onMount } from 'svelte';

  interface Props {
    /** HF `pipeline_tag`; '' = any task. */
    value: string;
    onchange: (task: string) => void;
  }

  let { value, onchange }: Props = $props();

  let open = $state(false);

  onMount(() => void taskSupportStore.load());

  const SUPPORT: Record<TaskSupport, { key: TKey; dot: string }> = {
    yes: { key: 'hub.task_support_yes', dot: 'bg-success' },
    partial: { key: 'hub.task_support_partial', dot: 'bg-warning' },
    no: { key: 'hub.task_support_no', dot: 'bg-muted-foreground/40' },
  };

  let quick = $derived(QUICK_TASKS.map((id) => taskInfo(id)).filter((x): x is HubTask => !!x));
  let current = $derived(taskInfo(value));
  /** A selected task outside the quick chips still shows as a chip. */
  let extra = $derived(current && !QUICK_TASKS.includes(current.id) ? current : null);

  /** Verdict of the installed runtime for a task; unknown until the backend answered. */
  function level(task: HubTask): TaskSupport | null {
    return taskSupportStore.support(task.id);
  }

  function dot(task: HubTask): string {
    const l = level(task);
    return l ? SUPPORT[l].dot : 'bg-muted-foreground/20';
  }

  function tip(task: HubTask): string {
    const info = taskSupportStore.info(task.id);
    if (!info) return task.hf;
    const parts = [task.hf, t(SUPPORT[info.support].key)];
    if (info.needs_runtime) parts.push(t('lib.runtime_needs', { runtime: info.needs_runtime }));
    else if (info.runtime) parts.push(info.runtime);
    if (info.runtime_missing) parts.push(t('hub.task_runtime_missing', { flag: info.flag ?? info.feature ?? '' }));
    else if (info.verified) parts.push(t('hub.task_runtime_verified'));
    return parts.join(' · ');
  }

  function pick(id: string) {
    open = false;
    if (id !== value) onchange(id);
  }

  const chip =
    'inline-flex h-7 shrink-0 items-center gap-1.5 rounded-md border px-2.5 text-xs transition-colors focus-visible:ring-2 focus-visible:ring-ring/50 focus-visible:outline-none';
  const chipIdle = 'border-border text-muted-foreground hover:bg-muted hover:text-foreground';
  const chipActive = 'border-primary/50 bg-primary/10 text-primary';
</script>

<div class="flex min-w-0 flex-wrap items-center gap-1.5" role="group" aria-label={t('hub.task_label')}>
  <span class="mr-0.5 text-xs text-muted-foreground">{t('hub.task_label')}</span>
  <FieldHelp text={t('help.task_all')}><button type="button" class={cn(chip, value === '' ? chipActive : chipIdle)} aria-pressed={value === ''} onclick={() => pick('')}>
    {t('hub.filter_all')}
  </button></FieldHelp>
  {#each quick as task (task.id)}
    {@const active = value === task.id}
    <button type="button" class={cn(chip, active ? chipActive : chipIdle)} aria-pressed={active} title={tip(task)} onclick={() => pick(task.id)}>
      <span class={cn('size-1.5 rounded-full', dot(task))} aria-hidden="true"></span>
      {t(task.key)}
    </button>
  {/each}
  {#if extra}
    <button type="button" class={cn(chip, chipActive)} aria-pressed="true" title={tip(extra)} onclick={() => (open = true)}>
      <span class={cn('size-1.5 rounded-full', dot(extra))} aria-hidden="true"></span>
      {t(extra.key)}
    </button>
  {/if}

  <Popover.Root bind:open>
    <Popover.Trigger>
      {#snippet child({ props })}
        <button {...props} type="button" class={cn(chip, chipIdle)}>
          <LayoutGrid class="size-3.5" />
          {t('hub.tasks_browse', { count: TASKS.length })}
          <ChevronDown class="size-3.5" />
        </button>
      {/snippet}
    </Popover.Trigger>
    <Popover.Content align="start" class="flex max-h-[min(70vh,520px)] w-[min(92vw,640px)] flex-col gap-3 overflow-y-auto p-3">
      <div class="flex flex-wrap items-center gap-x-3 gap-y-1 text-xs text-muted-foreground">
        {#each Object.entries(SUPPORT) as [level, s] (level)}
          <span class="inline-flex items-center gap-1.5">
            <span class={cn('size-1.5 rounded-full', s.dot)} aria-hidden="true"></span>{t(s.key)}
          </span>
        {/each}
      </div>
      <div class="grid grid-cols-1 gap-3 sm:grid-cols-2">
        {#each TASK_GROUPS as group (group.id)}
          <section class="flex min-w-0 flex-col gap-1" aria-label={t(group.key)}>
            <h3 class="text-xs font-semibold text-foreground">{t(group.key)}</h3>
            <div class="flex flex-wrap gap-1">
              {#each TASKS.filter((x) => x.group === group.id) as task (task.id)}
                {@const active = value === task.id}
                <button
                  type="button"
                  class={cn(chip, 'h-6 px-2', active ? chipActive : chipIdle, level(task) === 'no' && !active && 'opacity-75')}
                  aria-pressed={active}
                  title={tip(task)}
                  onclick={() => pick(task.id)}
                >
                  <span class={cn('size-1.5 rounded-full', dot(task))} aria-hidden="true"></span>
                  {t(task.key)}
                </button>
              {/each}
            </div>
          </section>
        {/each}
      </div>
      <AdvancedOnly><p class="text-xs text-muted-foreground">{t('hub.tasks_note')}</p></AdvancedOnly>
    </Popover.Content>
  </Popover.Root>
</div>
