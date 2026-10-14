import { AudioLines, Binary, Image, ListOrdered, MessageSquare, Workflow } from '@lucide/svelte';
import type { ServeMode } from '#lib/types.js';
import type { SectionPage } from '../common/sections';

export const PLAYGROUND_MODES: (SectionPage & { id: ServeMode })[] = [
  { id: 'chat', label: 'emb.tab_chat', icon: MessageSquare },
  { id: 'embedding', label: 'emb.tab_embedding', icon: Binary, level: 'advanced' },
  { id: 'rerank', label: 'emb.tab_rerank', icon: ListOrdered, level: 'advanced' },
  { id: 'transcription', label: 'emb.tab_transcription', icon: AudioLines },
  { id: 'image', label: 'img.tab', icon: Image },
  { id: 'pipeline', label: 'pl.tab', icon: Workflow, level: 'advanced' },
];

export function playgroundModeFromUrl(href: string): ServeMode | undefined {
  const url = new URL(href);
  const hash = url.hash.match(/^#playground\/(.+)$/)?.[1];
  const screen = url.searchParams.get('screen') ?? url.searchParams.get('tab');
  const id = hash ?? (screen === 'playground' ? url.searchParams.get('mode') ?? url.searchParams.get('section') : null);
  return PLAYGROUND_MODES.find(mode => mode.id === id)?.id;
}
