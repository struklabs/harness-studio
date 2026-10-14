<script lang="ts">
  // Offers the bundled `harness` CLI on the user's PATH (same backend as Settings > Developer > CLI).
  // Nothing is changed here: the wizard applies the choice when it finishes. Hidden when there is
  // no CLI binary, it is already reachable from new terminals, or it cannot be linked (inside a
  // running AppImage, or a translocated macOS app: the backend refuses those paths).
  import { onMount } from 'svelte';
  import { invoke } from '#lib/ipc.js';
  import { t } from '#lib/i18n.svelte.js';
  import { Switch } from '#lib/components/ui/switch/index.js';

  interface CliStatus {
    path: string | null;
    source: 'app' | 'path' | 'user' | null;
    mode: 'windows' | 'symlink';
    registered: boolean;
    /** macOS: the app runs from a temporary copy. */
    translocated: boolean;
    /** Linux: the CLI is inside the running AppImage's mount. */
    appimage: boolean;
  }

  interface Props {
    /** Whether the wizard should add the CLI to PATH on finish. */
    enabled: boolean;
  }

  let { enabled = $bindable() }: Props = $props();

  let offer = $state(false);

  onMount(async () => {
    try {
      const s = await invoke<CliStatus>('cli_status', { userPath: null });
      offer = Boolean(s.path) && !s.registered && s.source !== 'path' && !s.appimage && !s.translocated;
      // Changing PATH (and, on macOS, the shell profile) is a system change: always opt-in.
      enabled = false;
    } catch {
      offer = false;
      enabled = false;
    }
  });
</script>

{#if offer}
  <h2 class="mb-2 text-sm font-semibold">{t('onboarding.wizard_cli_path_section')}</h2>
  <div class="overflow-hidden rounded-lg border border-border bg-card">
    <div class="flex items-center justify-between gap-6 px-4 py-3">
      <label for="wiz-cli-path" class="flex min-w-0 flex-col gap-0.5">
        <span class="text-sm font-medium">{t('onboarding.wizard_cli_path_title')}</span>
        <span class="text-xs text-muted-foreground">{t('onboarding.wizard_cli_path_note')}</span>
      </label>
      <Switch id="wiz-cli-path" bind:checked={enabled} />
    </div>
  </div>
{/if}
