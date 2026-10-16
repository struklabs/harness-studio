import { test } from 'node:test';
import assert from 'node:assert/strict';
import { segmentTime, speakerLanes, segmentPosition } from '../src/lib/components/playground/diarization.ts';

const turn = (speaker_id, start, end, speaker = speaker_id) => ({ speaker_id, speaker, start, end });

test('segment timestamps round at minute boundaries and retain milliseconds', () => {
  assert.equal(segmentTime(0), '00:00.000');
  assert.equal(segmentTime(61.234), '01:01.234');
  assert.equal(segmentTime(59.9996), '01:00.000');
  assert.equal(segmentTime(6000), '100:00.000');
});

test('speaker lanes group by identity, preserve overlaps, and retain optional confidence', () => {
  const segments = [turn('one', 0, 2, 'A'), { ...turn('two', 1, 3, 'B'), confidence: 0.9 }, turn('one', 4, 5, 'A')];
  const lanes = speakerLanes(segments);
  assert.deepEqual(lanes.map(l => l.id), ['one', 'two']);
  assert.deepEqual(lanes[0].segments, [segments[0], segments[2]]);
  assert.equal(lanes[1].segments[0].confidence, 0.9);
  assert.equal(lanes[0].segments[0].confidence, undefined);
  assert.deepEqual(speakerLanes([]), []);
});

test('timeline bars use the full audio duration and stay within their lane', () => {
  assert.deepEqual(segmentPosition(turn('a', 2, 4), 10), { left: 20, width: 20 });
  assert.deepEqual(segmentPosition(turn('a', -1, 20), 10), { left: 0, width: 100 });
  assert.deepEqual(segmentPosition(turn('a', 4, 2), 10), { left: 40, width: 0 });
  assert.deepEqual(segmentPosition(turn('a', 0, 1), 0), { left: 0, width: 0 });
});
