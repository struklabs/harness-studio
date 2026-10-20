// Files the Playground makes (images, speech) go to the active project's outputs folder.
import { toast } from 'svelte-sonner';
import { t } from '#lib/i18n.svelte.js';
import { errorText } from '#lib/errors.js';
import { projectApi, type OutputSource } from '#lib/projects.js';
import { projects } from '#lib/stores/projects.svelte.js';

/** A short, file-safe name from a prompt: `a-red-fox.png`. */
export function outputName(prompt: string, ext: string, fallback = 'output'): string {
  const slug = prompt.toLowerCase().normalize('NFKD').replace(/[^\p{L}\p{N}]+/gu, '-').replace(/^-+|-+$/g, '').slice(0, 48);
  return `${slug || fallback}.${ext}`;
}

/** Saves into the active project when there is one with a folder; otherwise does nothing. */
export async function fileToProject(name: string, base64: string, from: OutputSource): Promise<void> {
  const project = projects.active;
  if (!project?.folder) return;
  try {
    await projectApi.saveOutput(project.id, name, base64, from);
    toast.success(t('proj.output_saved', { project: project.name }));
  } catch (e) {
    toast.error(errorText(e));
  }
}

export async function blobToBase64(blob: Blob): Promise<string> {
  const bytes = new Uint8Array(await blob.arrayBuffer());
  let binary = '';
  for (let i = 0; i < bytes.length; i += 0x8000) binary += String.fromCharCode(...bytes.subarray(i, i + 0x8000));
  return btoa(binary);
}
