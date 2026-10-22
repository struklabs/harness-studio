//! Wire limits and the key-less structural frame check of APP_PROTOCOL 6 and 7.5.
//! A port of `validateRelayFrame` / `LIMITS` in struklabs-web `packages/app-protocol`.

/// Padding bucket of a transport record; hides per-token delta sizes from the relay.
pub const RECORD_PAD: usize = 256;
/// Most data bytes in one record.
pub const RECORD_DATA_MAX: usize = 16384;
/// Most fragments of one reassembled envelope.
pub const MAX_FRAGMENTS: usize = 128;
/// AEAD tag of a Noise transport message.
pub const TAG: usize = 16;
/// Largest transport Noise message: the largest padded record plus the tag (16 656).
pub const MAX_TRANSPORT_MESSAGE: usize = padded_record_len(RECORD_DATA_MAX) + TAG;
/// Largest frame body: kind (1) + session (16) + Noise message (16 673).
pub const MAX_FRAME_BODY: usize = 17 + MAX_TRANSPORT_MESSAGE;
/// Largest whole frame including the u32 length prefix (16 677).
pub const MAX_FRAME: usize = 4 + MAX_FRAME_BODY;
/// Exact lengths of the three XX handshake messages with empty payloads.
pub const HANDSHAKE_LENGTHS: [usize; 3] = [32, 96, 64];
pub const KIND_HANDSHAKE: u8 = 1;
pub const KIND_TRANSPORT: u8 = 2;

/// The one valid record length for `data_len` bytes of data: header plus data, padded to a bucket.
pub const fn padded_record_len(data_len: usize) -> usize {
    (3 + data_len).div_ceil(RECORD_PAD) * RECORD_PAD
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Direction {
    BrowserToDesktop,
    DesktopToBrowser,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Refusal {
    BadFrame,
    PlaintextRefused,
    FrameTooLarge,
}

impl Refusal {
    pub fn code(self) -> &'static str {
        match self {
            Self::BadFrame => "relay.bad_frame",
            Self::PlaintextRefused => "relay.plaintext_refused",
            Self::FrameTooLarge => "relay.frame_too_large",
        }
    }
}

/// The check a relay applies to every binary frame without any key. `handshake_index` is the
/// number of handshake frames already forwarded on the session (0..=2); transport frames are only
/// legal after 3. `direction` also enforces the XX order: frames 0 and 2 come from the browser
/// (initiator), frame 1 from the desktop (responder).
pub fn validate_relay_frame(
    kind: u8,
    noise_len: usize,
    handshake_index: usize,
    direction: Option<Direction>,
) -> Result<(), Refusal> {
    if noise_len > MAX_TRANSPORT_MESSAGE {
        return Err(Refusal::FrameTooLarge);
    }
    match kind {
        KIND_HANDSHAKE => {
            if handshake_index > 2 {
                return Err(Refusal::BadFrame);
            }
            if let Some(d) = direction {
                if (d == Direction::BrowserToDesktop) != (handshake_index != 1) {
                    return Err(Refusal::BadFrame);
                }
            }
            if noise_len != HANDSHAKE_LENGTHS[handshake_index] {
                return Err(Refusal::PlaintextRefused);
            }
            Ok(())
        }
        KIND_TRANSPORT => {
            if handshake_index < 3 {
                return Err(Refusal::BadFrame);
            }
            // The tag plus whole padding buckets of record (at least one).
            let body = noise_len.saturating_sub(TAG);
            if noise_len < TAG || body < RECORD_PAD || !body.is_multiple_of(RECORD_PAD) {
                return Err(Refusal::PlaintextRefused);
            }
            Ok(())
        }
        _ => Err(Refusal::PlaintextRefused),
    }
}

/// `u32 BE body length || kind (1) || session (16) || Noise message`.
pub fn encode_frame(kind: u8, session: &[u8; 16], noise: &[u8]) -> Result<Vec<u8>, Refusal> {
    let body = 17 + noise.len();
    if body > MAX_FRAME_BODY {
        return Err(Refusal::FrameTooLarge);
    }
    let mut out = Vec::with_capacity(4 + body);
    out.extend_from_slice(&(body as u32).to_be_bytes());
    out.push(kind);
    out.extend_from_slice(session);
    out.extend_from_slice(noise);
    Ok(out)
}

/// Strict decode of exactly one frame: length, trailing bytes, kind and the transport bound.
/// Returns `(kind, session, noise message)`.
pub fn decode_frame(bytes: &[u8]) -> Result<(u8, [u8; 16], &[u8]), Refusal> {
    if bytes.len() < 4 {
        return Err(Refusal::BadFrame);
    }
    let body = u32::from_be_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]) as usize;
    if body > MAX_FRAME_BODY {
        return Err(Refusal::FrameTooLarge);
    }
    if bytes.len() != 4 + body || body < 17 {
        return Err(Refusal::BadFrame);
    }
    let kind = bytes[4];
    if kind != KIND_HANDSHAKE && kind != KIND_TRANSPORT {
        return Err(Refusal::PlaintextRefused);
    }
    if kind == KIND_TRANSPORT && body - 17 > MAX_TRANSPORT_MESSAGE {
        return Err(Refusal::FrameTooLarge);
    }
    let mut session = [0u8; 16];
    session.copy_from_slice(&bytes[5..21]);
    Ok((kind, session, &bytes[21..]))
}

/// Splits one envelope into padded records: `flags (1) || data length (u16 BE) || data || zeros`,
/// padded to a whole 256-byte bucket. Bit 0 of `flags` means more fragments follow.
pub fn split_into_records(message: &[u8]) -> Vec<Vec<u8>> {
    if message.is_empty() {
        return vec![vec![0; padded_record_len(0)]];
    }
    let count = message.len().div_ceil(RECORD_DATA_MAX);
    message
        .chunks(RECORD_DATA_MAX)
        .enumerate()
        .map(|(i, data)| {
            let mut record = vec![0; padded_record_len(data.len())];
            record[0] = u8::from(i + 1 < count);
            record[1..3].copy_from_slice(&(data.len() as u16).to_be_bytes());
            record[3..3 + data.len()].copy_from_slice(data);
            record
        })
        .collect()
}

/// A record broke the padding, flag, fragment or size rules: fatal for the session.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BadRecord;

/// Reassembles records into envelopes; any violation is fatal and clears the buffer.
#[derive(Default)]
pub struct RecordAssembler {
    assembled: Vec<u8>,
    fragments: usize,
}

impl RecordAssembler {
    pub fn push(
        &mut self,
        record: &[u8],
        max_envelope: usize,
    ) -> Result<Option<Vec<u8>>, BadRecord> {
        let result = self.accept(record, max_envelope);
        if result.is_err() {
            self.clear();
        }
        result
    }

    pub fn clear(&mut self) {
        self.assembled.clear();
        self.fragments = 0;
    }

    fn accept(&mut self, record: &[u8], max_envelope: usize) -> Result<Option<Vec<u8>>, BadRecord> {
        // Padding is checked only after the AEAD, so there is no padding oracle.
        if record.len() < 3 || record[0] > 1 {
            return Err(BadRecord);
        }
        let len = u16::from_be_bytes([record[1], record[2]]) as usize;
        let more = record[0] == 1;
        if len > RECORD_DATA_MAX
            || record.len() != padded_record_len(len)
            || record[3 + len..].iter().any(|b| *b != 0)
            // Every non-final fragment is a full chunk; this caps count and overhead.
            || (more && len != RECORD_DATA_MAX)
            || self.fragments >= MAX_FRAGMENTS
            || self.assembled.len() + len > max_envelope
        {
            return Err(BadRecord);
        }
        self.fragments += 1;
        self.assembled.extend_from_slice(&record[3..3 + len]);
        if more {
            Ok(None)
        } else {
            self.fragments = 0;
            Ok(Some(std::mem::take(&mut self.assembled)))
        }
    }
}
