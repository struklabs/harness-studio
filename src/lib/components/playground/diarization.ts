export interface SpeakerSegment {
  start: number;
  end: number;
  speaker: string;
  speaker_id: string;
  confidence?: number;
}

export interface DiarizationResult {
  model: string;
  task: 'diarization';
  duration: number;
  speakers: number;
  segments: SpeakerSegment[];
  outputs: [];
}

/** Round once to milliseconds so seconds never display as 60.000. */
export function segmentTime(seconds: number): string {
  const ms = Math.max(0, Math.round(seconds * 1000));
  return `${String(Math.floor(ms / 60000)).padStart(2, '0')}:${String(Math.floor(ms / 1000) % 60).padStart(2, '0')}.${String(ms % 1000).padStart(3, '0')}`;
}

/** Separate lanes preserve simultaneous speakers; identity comes from speaker_id. */
export function speakerLanes(segments: SpeakerSegment[]) {
  const lanes = new Map<string, { id: string; speaker: string; segments: SpeakerSegment[] }>();
  for (const segment of segments) {
    let lane = lanes.get(segment.speaker_id);
    if (!lane) {
      lane = { id: segment.speaker_id, speaker: segment.speaker, segments: [] };
      lanes.set(segment.speaker_id, lane);
    }
    lane.segments.push(segment);
  }
  return [...lanes.values()];
}

export function segmentPosition(segment: SpeakerSegment, duration: number) {
  if (!(duration > 0)) return { left: 0, width: 0 };
  const start = Math.max(0, Math.min(duration, segment.start));
  const end = Math.max(start, Math.min(duration, segment.end));
  return { left: start / duration * 100, width: (end - start) / duration * 100 };
}
