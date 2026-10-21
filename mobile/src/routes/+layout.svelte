<script lang="ts">
  import '../app.css';
  import { setLanguage } from '#lib/i18n.svelte.js';
  import type { Snippet } from 'svelte';

  let { children }: { children: Snippet } = $props();

  // The phone's language and light/dark setting.
  setLanguage(typeof navigator !== 'undefined' && navigator.language?.toLowerCase().startsWith('tr') ? 'tr' : 'en');
  $effect(() => {
    const media = window.matchMedia('(prefers-color-scheme: dark)');
    const apply = () => {
      const scheme = media.matches ? 'dark' : 'light';
      document.documentElement.dataset.theme = scheme;
      document.documentElement.dataset.scheme = scheme;
    };
    apply();
    media.addEventListener('change', apply);
    return () => media.removeEventListener('change', apply);
  });
</script>

{@render children()}
