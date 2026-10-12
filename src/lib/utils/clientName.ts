import { t } from '$lib/i18n.svelte';

export function getClientName(client: string | null | undefined): string {
  if (!client) return t('an.client_unknown');
  switch (client) {
    case 'Harness Playground':
    case 'harness-playground': return t('an.client_playground');
    case 'harness-auto-tools': return t('an.client_auto_tools');
    case 'harness-tool-creator': return t('an.client_tool_creator');
    case 'harness-cli': return t('an.client_cli');
    case 'direct_usage': return t('an.direct_title');
    case 'harness-launch': return t('an.client_launcher');
    default: return client;
  }
}
