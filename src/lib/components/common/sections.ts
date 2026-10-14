import type { Cpu } from '@lucide/svelte';
import type { TKey } from '#lib/i18n.svelte.js';
export interface SectionPage { level?: 'advanced'; id: string; label: TKey; icon: typeof Cpu }
export interface SectionGroup { level?: 'advanced'; label?: TKey; pages: SectionPage[] }
