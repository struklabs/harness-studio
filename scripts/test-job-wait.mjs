import { test } from 'node:test';
import assert from 'node:assert/strict';
import { aliasSlug, queuedJobFor, waitLine } from '../src/lib/components/jobWait.ts';

const job = (label, extra = {}) => ({ id: 1, label, priority: 'interactive', status: 'queued', memory_bytes: 4659398685, queue_wait_ms: 1000, load_ms: null, ...extra });

test('a model alias maps to its server id the way the orchestrator names it', () => {
  assert.equal(aliasSlug('Wan2.1-T2V-1.3B-Q4_K_M'), 'wan2-1-t2v-1-3b-q4-k-m');
  assert.equal(aliasSlug('flux2-klein-4b-mflux-q3'), 'flux2-klein-4b-mflux-q3');
  assert.equal(aliasSlug('***'), 'model');
});

test('the studio finds the queued job that starts its model, not another one', () => {
  const jobs = [job('m-other'), job('gw-flux2-klein-4b-mflux-q3', { id: 2 }), job('m-wan2-1-t2v-1-3b-q4-k-m-2', { id: 3 })];
  assert.equal(queuedJobFor(jobs, 'flux2-klein-4b-mflux-q3')?.id, 2);
  assert.equal(queuedJobFor(jobs, 'Wan2.1-T2V-1.3B-Q4_K_M')?.id, 3);
  assert.equal(queuedJobFor(jobs, 'flux2-klein'), undefined);
  assert.equal(queuedJobFor([job('gw-a', { status: 'loading' })], 'a'), undefined);
});

test('the wait line names the reason with need, free and budget', () => {
  const wait = { reason: 'memory', free_bytes: 4190000000, capacity_bytes: 5720000000 };
  assert.deepEqual(waitLine(job('gw-a', { wait })), { key: 'jobs.wait_memory', need: 4659398685, free: 4190000000, capacity: 5720000000 });
  assert.equal(waitLine(job('gw-a', { wait: { ...wait, reason: 'queue' } })).key, 'jobs.wait_queue');
  assert.equal(waitLine(job('gw-a', { wait: { ...wait, reason: 'alone' }, memory_bytes: null })).need, null);
  // Older servers send no wait; a started job has none.
  assert.equal(waitLine(job('gw-a')), null);
  assert.equal(waitLine(job('gw-a', { wait, status: 'loading' })), null);
});
