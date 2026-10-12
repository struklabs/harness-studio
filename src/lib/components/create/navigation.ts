import { Hammer, Workflow, Wrench } from '@lucide/svelte';
import type { SectionGroup, SectionPage } from '../common/sections';
export const CREATE_PAGES: SectionPage[] = [
  { id: 'generator', label: 'create.generator', icon: Hammer },
  { id: 'auto', label: 'create.auto', icon: Workflow },
  { id: 'mine', label: 'create.mine', icon: Wrench },
];
export const CREATE_GROUPS: SectionGroup[] = [{ pages: CREATE_PAGES }];
