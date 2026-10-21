import type { Character } from '#lib/office/scene.js';
/** The mounted Playground retains ownership of its approval broker. */
export const officeLive = $state<{character:Character|null; open:(()=>void)|null}>({character:null,open:null});
