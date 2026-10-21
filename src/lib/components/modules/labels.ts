import Mic from '@lucide/svelte/icons/mic';
import Eye from '@lucide/svelte/icons/eye';
import Bot from '@lucide/svelte/icons/bot';
import GraduationCap from '@lucide/svelte/icons/graduation-cap';
import Workflow from '@lucide/svelte/icons/workflow';
import Database from '@lucide/svelte/icons/database';
import Plug from '@lucide/svelte/icons/plug';
import CodeXml from '@lucide/svelte/icons/code-xml';
import Puzzle from '@lucide/svelte/icons/puzzle';
import { t, type TKey } from '#lib/i18n.svelte.js';

const ICONS = { speech: Mic, vision: Eye, agents: Bot, training: GraduationCap, automation: Workflow, data: Database, integrations: Plug, developer: CodeXml } as const;
export const categoryIcon = (category: string) => ICONS[category as keyof typeof ICONS] ?? Puzzle;

const titleCase = (id: string) => id.split(/[-_\s]+/).filter(Boolean).map(w => w.charAt(0).toUpperCase() + w.slice(1)).join(' ');
/** A translated label when the key exists, else the id in Title Case (registry categories the app does not know yet). */
function labelOr(key: string, id: string): string {
  const text = t(key as TKey);
  return text === key ? titleCase(id) : text;
}
export const categoryLabel = (id: string) => labelOr(`modules.category_${id}`, id);
export const capabilityLabel = (id: string) => labelOr(`modules.capability_${id}`, id);
/** `windows-x86_64` reads "Windows x86_64"; operating system names are proper nouns. */
export function platformLabel(id: string): string {
  const [os, arch] = id.split('-');
  const name = os === 'macos' ? 'macOS' : titleCase(os);
  return arch ? `${name} ${arch}` : name;
}
