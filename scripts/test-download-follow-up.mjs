import assert from 'node:assert/strict';
import { test } from 'node:test';
const { repoJobsPending, shouldAutoLoad } = await import('../src/lib/downloadFollowUp.ts');

const llm = { is_ready: true, kind: 'llm', filename: 'SmolVLM-256M-Instruct-Q8_0.gguf', gguf: {} };
const job = { id: 'a', repo_id: 'ggml-org/SmolVLM-256M-Instruct-GGUF' };

test('a model is not offered for loading while its projector is still downloading', () => {
  const projector = { id: 'b', repo_id: job.repo_id, status: 'Downloading' };
  assert.equal(repoJobsPending(job, [{ ...job, status: 'Completed' }, projector]), true);
  assert.equal(shouldAutoLoad(true, job, llm, [projector]), false);
  assert.equal(repoJobsPending(job, [{ ...projector, status: 'Completed' }]), false);
  assert.equal(shouldAutoLoad(true, job, llm, [{ ...projector, status: 'Completed' }]), true);
});

test('jobs of other repos and the finished job itself do not hold the offer back', () => {
  assert.equal(repoJobsPending(job, [{ ...job, status: 'Downloading' }]), false);
  assert.equal(repoJobsPending(job, [{ id: 'c', repo_id: 'other/repo', status: 'Queued' }]), false);
});
