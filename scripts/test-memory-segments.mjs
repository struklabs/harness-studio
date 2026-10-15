// Memory bar split: a server with nothing on the device (the iGPU while the model runs on the NVIDIA card) is not listed.
import { test } from 'node:test';
import assert from 'node:assert/strict';

const { buildSegments } = await import('../src/lib/memorySegments.ts');

const MiB = 1024 * 1024;
const server = (id, bytes) => ({ id, name: `${id}.gguf`, bytes });

test('servers with 0 bytes on this device are left out of the bar and the legend', () => {
  const bar = buildSegments(0, 128 * MiB, { harness_bytes: 0, other_bytes: 0, servers: [server('gw-a', 0), server('gw-b', 0)] });
  assert.deepEqual(bar.segments.map((s) => s.kind), ['harness', 'other']);
  assert.equal(bar.freeBytes, 128 * MiB);
});

test('servers that use the device keep their segment', () => {
  const bar = buildSegments(3500 * MiB, 4096 * MiB, { harness_bytes: 0, other_bytes: 13 * MiB, servers: [server('gw-a', 3485 * MiB), server('gw-b', 0)] });
  assert.deepEqual(bar.segments.map((s) => s.key), ['harness', 'server:gw-a', 'other']);
  assert.ok(bar.segments[1].pct > 80);
});
