import { Activity, Cpu, Gauge, Globe, HardDrive, Network, Settings2, SlidersHorizontal, Wrench, Users } from '@lucide/svelte';
import type { SectionGroup } from './sections';

export const DASHBOARD_GROUPS: SectionGroup[] = [
  { label: 'menus.server', pages: [
    { id: 'overview', label: 'menus.overview', icon: Activity },
    { id: 'model', simpleLabel: 'uiLevel.model', label: 'menus.model_runtime', icon: Cpu },
    { level: 'advanced', id: 'network', label: 'menus.network', icon: Network },
  ] },
  { label: 'menus.inference', pages: [
    { id: 'context', simpleLabel: 'uiLevel.context', label: 'menus.context_memory', icon: HardDrive },
    { level: 'advanced', id: 'gpu', label: 'menus.gpu', icon: Cpu },
    { level: 'advanced', id: 'sampling', label: 'menus.sampling', icon: SlidersHorizontal },
  ] },
  { level: 'advanced', label: 'menus.advanced', pages: [
    { level: 'advanced', id: 'template', label: 'menus.template', icon: Settings2 },
    { level: 'advanced', id: 'tools', label: 'menus.tools', icon: Wrench },
    { level: 'advanced', id: 'other', label: 'menus.other', icon: Gauge },
  ] },
];

export const PLAYGROUND_GROUPS: SectionGroup[] = [
  { label: 'menus.session', pages: [
    { id: 'model', label: 'menus.model_preset', icon: Cpu },
    { id: 'system', simpleLabel: 'uiLevel.system', label: 'menus.system', icon: Settings2 },
  ] },
  { level: 'advanced', label: 'menus.generation', pages: [
    { level: 'advanced', id: 'sampling', label: 'menus.sampling', icon: SlidersHorizontal },
    { level: 'advanced', id: 'output', label: 'menus.output', icon: Gauge },
  ] },
  { level: 'advanced', label: 'menus.context', pages: [
    { level: 'advanced', id: 'history', label: 'menus.history', icon: HardDrive },
    { level: 'advanced', id: 'rag', label: 'menus.rag', icon: Globe },
  ] },
  { label: 'menus.tools', pages: [{ id: 'tools', label: 'menus.permissions', icon: Wrench }] },
];

export const LIBRARY_GROUPS: SectionGroup[] = [
  { label: 'menus.models', pages: [
    { id: 'all', label: 'lib.tab_all', icon: Cpu },
    { id: 'llm', label: 'menus.chat', icon: Cpu },
    { id: 'vlm', label: 'menus.vision', icon: Cpu },
    { level: 'advanced', id: 'embedding', label: 'lib.tab_embedding', icon: Cpu },
    { level: 'advanced', id: 'reranker', label: 'lib.tab_reranker', icon: Cpu },
    { id: 'asr', label: 'menus.speech', icon: Cpu },
    { id: 'image', label: 'menus.image', icon: Cpu },
    { id: 'tts', label: 'lib.tab_tts', icon: Cpu },
  ] },
  { level: 'advanced', label: 'menus.files', pages: [
    { level: 'advanced', id: 'draft', label: 'menus.draft', icon: HardDrive },
    { level: 'advanced', id: 'adapter', label: 'menus.adapter', icon: HardDrive },
    { level: 'advanced', id: 'mmproj', label: 'menus.projector', icon: HardDrive },
  ] },
  { label: 'menus.management', pages: [
    { id: 'import', label: 'menus.import', icon: Globe },
    { level: 'advanced', id: 'definitions', label: 'menus.definitions', icon: Settings2 },
  ] },
];

export const PERFORMANCE_GROUPS: SectionGroup[] = [
  { label: 'menus.live', pages: [
    { id: 'overview', label: 'menus.overview', icon: Activity },
    { id: 'speed', label: 'menus.speed', icon: Gauge },
    { id: 'resources', label: 'menus.resources', icon: Cpu },
    { id: 'clients', label: 'an.clients', icon: Users },
  ] },
  { label: 'menus.benchmark', pages: [
    { id: 'new', label: 'menus.new_benchmark', icon: SlidersHorizontal },
    { id: 'results', label: 'menus.results', icon: HardDrive },
    { id: 'compare', label: 'menus.compare', icon: Activity },
  ] },
];

export const ANALYTICS_GROUPS: SectionGroup[] = [
  { label: 'menus.analytics', pages: [
    { id: 'overview', label: 'an.tab_overview', icon: Activity },
    { id: 'breakdown', label: 'menus.breakdown', icon: Gauge },
  ] },
  { label: 'menus.requests', pages: [
    { id: 'requests', label: 'menus.log', icon: HardDrive },
    { id: 'detail', label: 'menus.detail', icon: Settings2 },
  ] },
  { label: 'menus.retention', pages: [{ id: 'policy', label: 'menus.policy', icon: SlidersHorizontal }] },
];
