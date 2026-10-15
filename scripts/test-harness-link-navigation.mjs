import test from 'node:test';
import assert from 'node:assert/strict';
import { resolveConnectionPage, connectionPageFromUrl } from '../src/lib/components/connections/navigationResolver.js';
import { filterRemoteModels, isRemoteModel } from '../src/lib/components/playground/remoteModels.ts';
import { gatewayModelStates } from '../src/lib/components/playground/gatewayModels.ts';
import { selectChatModel } from '../src/lib/components/playground/modelSelection.ts';

test('remote connections page resolves from saved navigation and deep links', () => {
  assert.equal(resolveConnectionPage('harness-link'), 'harness-link');
  assert.equal(connectionPageFromUrl('https://localhost/?connection=harness-link'), 'harness-link');
  assert.equal(connectionPageFromUrl('https://localhost/#connections/harness-link'), 'harness-link');
  assert.equal(resolveConnectionPage('clients'), 'config');
  assert.equal(resolveConnectionPage('unknown'), null);
});

test('remote picker search covers link names and namespaced model ids, retaining offline rows', () => {
  const models = [
    { id: 'remote:office/org/model', model: 'org/model', link: 'office', state: 'online' },
    { id: 'remote:home/other', model: 'other', link: 'home', state: 'offline' },
  ];
  assert.deepEqual(filterRemoteModels(models, ' OFFICE org/Model '), [models[0]]);
  assert.deepEqual(filterRemoteModels(models, 'remote:home/other'), [models[1]]);
  assert.deepEqual(filterRemoteModels(models, ''), models);
  assert.deepEqual(filterRemoteModels(models, 'missing'), []);
  assert.equal(isRemoteModel(models[0].id), true);
  assert.equal(isRemoteModel('local-model'), false);
});

test('remote models bypass local load badges and retain an explicit chat selection', () => {
  const remote = 'remote:office/org/model';
  const states = gatewayModelStates([
    { id: remote, owned_by: 'harness-link', harness: { state: 'unloaded' } },
    { id: 'external', owned_by: 'harness-link' },
    { id: 'remote:home/other' },
    { id: 'local', harness: { definition_id: 'def', state: 'unloaded' } },
    { id: 'loading', harness: { state: 'loading' } },
    { id: 'idle' },
  ], (id) => id === 'def' ? 'running' : undefined);
  assert.deepEqual(states, { local: 'loaded', loading: 'loading', idle: 'unloaded' });
  assert.equal(selectChatModel(remote, ['local', remote], states, 'local'), remote);
  assert.equal(selectChatModel('idle', ['idle', 'local'], states, 'local'), 'local');
});
