import type { Character, OfficeStatus } from '../../src/lib/office/scene.ts';

/** A repeatable busy office, shared by positioning tests and browser perf measurements. */
export function officeAgents(): Character[] {
  const statuses: OfficeStatus[] = ['thinking', 'running_tool', 'waiting_for_approval', 'idle', 'error', 'sleeping'];
  return Array.from({ length: 30 }, (_, i) => ({
    id: `taskflow-fixture-${i}`, name: `Agent ${i}`, kind: 'agent', status: statuses[i % statuses.length],
    run_id: `taskflow-fixture-${i}`, agent: `fixture-model-${i % 3}`,
  }));
}
