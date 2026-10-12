//! A small Server-Sent Events parser (the `text/event-stream` of Streamable HTTP responses).
//!
//! Only what MCP needs: `data:` lines joined by `\n`, `event:` and `id:` fields, comment lines
//! (`:` keep-alives) ignored, `\n`, `\r\n` and `\r` line ends, events split across chunks.

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SseEvent {
    pub event: Option<String>,
    pub id: Option<String>,
    pub data: String,
}

#[derive(Default)]
pub struct SseParser {
    buf: Vec<u8>,
    cur: SseEvent,
    has_data: bool,
    /// The previous chunk ended with `\r`: a following `\n` belongs to the same line end.
    skip_lf: bool,
}

impl SseParser {
    pub fn new() -> Self {
        Self::default()
    }

    /// Feeds bytes; returns the events completed by them.
    pub fn push(&mut self, chunk: &[u8]) -> Vec<SseEvent> {
        let mut out = Vec::new();
        for &b in chunk {
            if self.skip_lf {
                self.skip_lf = false;
                if b == b'\n' {
                    continue;
                }
            }
            match b {
                b'\n' => self.end_line(&mut out),
                b'\r' => {
                    self.skip_lf = true;
                    self.end_line(&mut out);
                }
                _ => self.buf.push(b),
            }
        }
        out
    }

    fn end_line(&mut self, out: &mut Vec<SseEvent>) {
        let raw = std::mem::take(&mut self.buf);
        let line = String::from_utf8_lossy(&raw);
        if line.is_empty() {
            // Blank line: dispatch.
            if self.has_data || self.cur.event.is_some() {
                out.push(std::mem::take(&mut self.cur));
            }
            self.cur = SseEvent::default();
            self.has_data = false;
            return;
        }
        if line.starts_with(':') {
            return;
        }
        let (field, value) = match line.split_once(':') {
            Some((f, v)) => (f, v.strip_prefix(' ').unwrap_or(v)),
            None => (line.as_ref(), ""),
        };
        match field {
            "data" => {
                if self.has_data {
                    self.cur.data.push('\n');
                }
                self.cur.data.push_str(value);
                self.has_data = true;
            }
            "event" => self.cur.event = Some(value.to_string()),
            "id" => self.cur.id = Some(value.to_string()),
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn data(events: &[SseEvent]) -> Vec<&str> {
        events.iter().map(|e| e.data.as_str()).collect()
    }

    #[test]
    fn events_split_across_chunks_and_line_ends() {
        let mut p = SseParser::new();
        let mut all = Vec::new();
        for chunk in [
            &b"id: 1\nevent: message\nda"[..],
            b"ta: {\"a\":1}\n\n: keep-alive\n\ndata: x\r\n",
            b"data: y\r\n\r\ndata: z\r",
            b"\n\r",
            b"\n",
        ] {
            all.extend(p.push(chunk));
        }
        assert_eq!(data(&all), ["{\"a\":1}", "x\ny", "z"]);
        assert_eq!(all[0].event.as_deref(), Some("message"));
        assert_eq!(all[0].id.as_deref(), Some("1"));
    }

    #[test]
    fn comments_alone_make_no_event() {
        let mut p = SseParser::new();
        assert!(p.push(b":\r\n\r\n: hi\n\n").is_empty());
    }
}
