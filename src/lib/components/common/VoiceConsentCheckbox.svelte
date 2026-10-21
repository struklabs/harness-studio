<script lang="ts">
  // The consent box for cloning a voice. Every screen that sends reference audio
  // (`voice_ref`) uses this one, so the wording and the rule stay the same: the request is
  // only sent once the person confirms they have the speaker's permission.
  import { t } from '#lib/i18n.svelte.js';
  import { Checkbox } from '#lib/components/ui/checkbox/index.js';
  import { Label } from '#lib/components/ui/label/index.js';

  let { id = 'voice-consent', checked = $bindable(false), disabled = false }: { id?: string; checked?: boolean; disabled?: boolean } = $props();
</script>

<div class="flex items-start gap-2">
  <Checkbox {id} {checked} onCheckedChange={(v) => (checked = v === true)} {disabled} class="mt-0.5" />
  <div class="flex flex-col gap-0.5">
    <Label for={id} class="text-sm font-medium text-foreground">{t('voice.consent_label')}</Label>
    <span class="text-xs text-muted-foreground">{t('voice.consent_note')}</span>
  </div>
</div>
