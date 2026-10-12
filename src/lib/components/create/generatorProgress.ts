import type { GeneratorAttempt, GeneratorUsage } from '$lib/tools-client';

export interface GeneratorProgress {
  history: GeneratorAttempt[];
  number: number;
  stage: 'writing' | 'testing' | 'retrying' | 'complete';
  result: { passed: number; total: number } | null;
  usage: GeneratorUsage;
}

export function emptyProgress(): GeneratorProgress {
  return { history: [], number: 1, stage: 'writing', result: null, usage: { prompt_tokens: 0, completion_tokens: 0, estimated: false } };
}

export function collectProgress(state: GeneratorProgress, progress: string): GeneratorProgress {
  if (progress.startsWith('build:')) {
    return { ...state, number: Number(progress.slice(6)), stage: 'writing' };
  }
  if (progress === 'test') return { ...state, stage: 'testing' };
  if (progress.startsWith('usage:')) {
    const usage = JSON.parse(progress.slice(6)) as GeneratorUsage;
    return { ...state, usage: {
      prompt_tokens: state.usage.prompt_tokens + usage.prompt_tokens,
      completion_tokens: state.usage.completion_tokens + usage.completion_tokens,
      estimated: state.usage.estimated || usage.estimated,
    } };
  }
  if (progress.startsWith('attempt:')) {
    const attempt = JSON.parse(progress.slice(8)) as GeneratorAttempt;
    return { ...state, number: attempt.number,
      stage: attempt.failed_cases > 0 || attempt.feedback.trim() ? 'retrying' : 'complete',
      result: attempt.total_cases === undefined ? null : { passed: attempt.total_cases - attempt.failed_cases, total: attempt.total_cases },
      history: [...state.history.filter(a => a.number !== attempt.number), attempt] };
  }
  return state;
}

/** Summarize failed cases without exposing model feedback in the card. */
export function testFailureSummary(report: { cases: { passed: boolean; reason: string; actual: string }[]; error: string | null }) {
  const failed = report.cases.filter(c => !c.passed);
  const missing = failed.filter(c => c.reason === 'unexpected_error' && c.actual.includes('path does not exist:')).length;
  return missing ? { key: 'create.test_missing_files' as const, count: missing }
    : failed.length ? { key: 'create.test_cases_failed' as const, count: failed.length }
    : { key: 'create.test_run_failed' as const, count: 0 };
}

export function showRepetitionWarning(attempt: GeneratorAttempt): boolean {
  return attempt.repeated && (attempt.failed_cases > 0 || !!attempt.feedback.trim());
}
