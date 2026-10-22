// Dev-only: synthetic Agent Office characters for checking the scene at scale.
// The page loads it only when `import.meta.env.DEV` is true and `?office_fixture=30`
// (or `localStorage.hs_office_fixture`) asks for it; production builds never import it.
import type { Character, OfficeStatus } from '../office/scene.ts';

const STATUSES: OfficeStatus[] = ['idle', 'thinking', 'running_tool', 'waiting_for_approval', 'error', 'sleeping'];

/** Requested character count (1–200), or 0 when no fixture is asked for. */
export function officeFixtureCount(search: string, stored: string | null): number {
  const raw = new URLSearchParams(search).get('office_fixture') ?? stored;
  const n = Number(raw);
  return Number.isInteger(n) && n > 0 ? Math.min(n, 200) : 0;
}

/** `count` agents; `tick` shifts every status so the scene keeps changing like live activity. */
export function fixtureCharacters(count: number, tick: number): Character[] {
  return Array.from({ length: count }, (_, i) => ({
    id: `fixture-${i}`,
    name: `Agent ${i + 1}`,
    kind: 'agent',
    status: STATUSES[(i + tick) % STATUSES.length],
    run_id: null,
    project_id: null,
  }));
}
