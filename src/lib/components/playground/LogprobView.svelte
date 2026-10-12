<script lang="ts">
  // Answer text as token spans tinted by how unsure the model was (1 - probability); the
  // tooltip lists the chosen token and its most likely alternatives.
  import { t, locale } from '#lib/i18n.svelte.js';
  import { formatProbability, uncertainty, type TokenLogprob } from './logprobs';

  interface Props {
    tokens: TokenLogprob[];
  }

  let { tokens }: Props = $props();

  const show = (s: string) => s.replace(/\n/g, '\\n');

  function tip(tok: TokenLogprob): string {
    const loc = locale();
    const lines = [t('pg.logprobs_token', { token: show(tok.token), prob: formatProbability(tok.logprob, loc) })];
    if (tok.top.length) {
      lines.push(t('pg.logprobs_alternatives'));
      for (const a of tok.top) lines.push(`${formatProbability(a.logprob, loc).padStart(6)}  ${show(a.token)}`);
    }
    return lines.join('\n');
  }

  function tint(tok: TokenLogprob): string {
    const u = uncertainty(tok.logprob);
    return u < 0.02 ? '' : `background-color: color-mix(in oklab, var(--warning, #d97706) ${Math.round(u * 70)}%, transparent)`;
  }
</script>

<div class="flex min-w-0 flex-col gap-1.5">
  <p class="m-0 text-xs text-muted-foreground" role="note">{t('pg.logprobs_legend')} {t('pg.logprobs_session_only')}</p>
  <p class="m-0 text-base leading-relaxed whitespace-pre-wrap text-foreground [overflow-wrap:anywhere]">
    {#each tokens as tok, i (i)}<span class="rounded-[2px]" style={tint(tok)} title={tip(tok)}>{tok.token}</span>{/each}
  </p>
</div>
