//! Splitting extracted text into overlapping chunks.
//!
//! Sizes are in characters (a token is ~3–4 characters of prose; the embedding servers
//! Harness starts take the model's training context as batch size, e.g. 2048 tokens for
//! nomic-embed, so [`MAX_CHARS`] stays far below any limit).
//!
//! * Paragraphs (blank-line separated) are the units; they are packed until [`TARGET_CHARS`].
//! * A Markdown heading (`#`–`######`, also produced from DOCX heading styles) starts a new
//!   chunk once the current one has [`MIN_CHARS`], and is remembered as the chunk's
//!   `heading` (`Parent › Child`).
//! * A paragraph longer than [`MAX_CHARS`] is cut at sentence ends, then at spaces.
//! * Consecutive chunks of one section share up to [`OVERLAP_CHARS`] (the previous chunk's
//!   tail, cut at a sentence or word boundary) so an answer split across a border is still
//!   found.
//! * Chunks carry the first and last page they cover (PDF).

use super::extract::PageText;
use serde::{Deserialize, Serialize};

pub const TARGET_CHARS: usize = 900;
pub const MAX_CHARS: usize = 1200;
pub const MIN_CHARS: usize = 200;
pub const OVERLAP_CHARS: usize = 150;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Chunk {
    pub text: String,
    pub page_start: Option<u32>,
    pub page_end: Option<u32>,
    pub heading: Option<String>,
}

struct Block {
    text: String,
    page: Option<u32>,
    /// Heading level (1–6) when the block is a Markdown heading line.
    level: Option<usize>,
}

fn clen(s: &str) -> usize {
    s.chars().count()
}

fn heading_level(line: &str) -> Option<(usize, &str)> {
    let hashes = line.chars().take_while(|c| *c == '#').count();
    if (1..=6).contains(&hashes) && line[hashes..].starts_with(' ') {
        let title = line[hashes..].trim();
        (!title.is_empty()).then_some((hashes, title))
    } else {
        None
    }
}

fn blocks(pages: &[PageText]) -> Vec<Block> {
    let mut out = Vec::new();
    for page in pages {
        for para in page.text.split("\n\n") {
            let para = para.trim_matches('\n');
            if para.trim().is_empty() {
                continue;
            }
            // A heading line followed by text in the same paragraph is split off.
            let mut rest = para;
            if let Some((first, tail)) = para.split_once('\n').or(Some((para, ""))) {
                if let Some((level, _)) = heading_level(first.trim_start()) {
                    out.push(Block {
                        text: first.trim().to_string(),
                        page: page.number,
                        level: Some(level),
                    });
                    rest = tail;
                }
            }
            if !rest.trim().is_empty() {
                out.push(Block {
                    text: rest.to_string(),
                    page: page.number,
                    level: None,
                });
            }
        }
    }
    out
}

/// Byte index at which `s` has `n` characters (or `s.len()`).
fn byte_at(s: &str, n: usize) -> usize {
    s.char_indices().nth(n).map(|(i, _)| i).unwrap_or(s.len())
}

/// Splits an over-long paragraph into pieces of at most [`MAX_CHARS`], preferring sentence
/// ends, then line breaks, then spaces.
fn split_long(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut rest = text.trim();
    while clen(rest) > MAX_CHARS {
        let window = &rest[..byte_at(rest, MAX_CHARS)];
        let min = byte_at(window, TARGET_CHARS / 2);
        let cut = [". ", "? ", "! ", "; ", "\n", " "]
            .iter()
            .find_map(|sep| {
                window
                    .rmatch_indices(sep)
                    .map(|(i, _)| i + sep.len())
                    .find(|&i| i > min)
            })
            .unwrap_or(window.len());
        out.push(rest[..cut].trim().to_string());
        rest = rest[cut..].trim_start();
    }
    if !rest.is_empty() {
        out.push(rest.to_string());
    }
    out
}

/// The last ~[`OVERLAP_CHARS`] of a chunk, starting at a sentence or word boundary.
fn tail(text: &str) -> &str {
    let n = clen(text);
    if n <= OVERLAP_CHARS {
        return "";
    }
    let from = byte_at(text, n - OVERLAP_CHARS);
    let window = &text[from..];
    let start = [". ", "? ", "! ", "\n"]
        .iter()
        .filter_map(|sep| window.find(sep).map(|i| i + sep.len()))
        .min()
        .filter(|&i| i < window.len() * 2 / 3)
        .or_else(|| window.find(' ').map(|i| i + 1))
        .unwrap_or(0);
    window[start..].trim()
}

struct Builder {
    chunks: Vec<Chunk>,
    text: String,
    first_page: Option<u32>,
    last_page: Option<u32>,
    /// Characters of real content (not the carried-over overlap).
    fresh: usize,
    headings: Vec<(usize, String)>,
}

impl Builder {
    fn heading(&self) -> Option<String> {
        (!self.headings.is_empty()).then(|| {
            self.headings
                .iter()
                .map(|(_, t)| t.as_str())
                .collect::<Vec<_>>()
                .join(" › ")
        })
    }

    fn push(&mut self, piece: &str, page: Option<u32>) {
        if !self.text.is_empty() {
            self.text.push_str("\n\n");
        }
        self.text.push_str(piece);
        self.fresh += clen(piece);
        if self.first_page.is_none() {
            self.first_page = page;
        }
        if page.is_some() {
            self.last_page = page;
        }
    }

    /// Ends the current chunk; `overlap` carries its tail into the next one.
    fn flush(&mut self, overlap: bool) {
        if self.fresh == 0 {
            return;
        }
        let text = std::mem::take(&mut self.text);
        let carried = if overlap {
            tail(&text).to_string()
        } else {
            String::new()
        };
        self.chunks.push(Chunk {
            text,
            page_start: self.first_page,
            page_end: self.last_page,
            heading: self.heading(),
        });
        self.fresh = 0;
        self.first_page = if carried.is_empty() {
            None
        } else {
            self.last_page
        };
        self.text = carried;
    }
}

/// Chunks of a document (in order).
pub fn chunk_pages(pages: &[PageText]) -> Vec<Chunk> {
    let mut b = Builder {
        chunks: Vec::new(),
        text: String::new(),
        first_page: None,
        last_page: None,
        fresh: 0,
        headings: Vec::new(),
    };
    for block in blocks(pages) {
        if let Some(level) = block.level {
            // A new section: close a chunk that is big enough (no overlap across sections).
            if b.fresh >= MIN_CHARS {
                b.flush(false);
            } else if b.fresh == 0 {
                // Only the previous section's overlap: drop it.
                b.text.clear();
                b.first_page = None;
            }
            b.headings.retain(|(l, _)| *l < level);
            let title = heading_level(&block.text)
                .map(|(_, t)| t.to_string())
                .unwrap_or_default();
            b.headings.push((level, title));
            b.push(&block.text, block.page);
            continue;
        }
        for piece in split_long(&block.text) {
            let n = clen(&piece);
            if b.fresh > 0 && clen(&b.text) + n + 2 > TARGET_CHARS {
                b.flush(true);
            }
            b.push(&piece, block.page);
        }
    }
    b.flush(false);
    b.chunks
}

/// Text sent to the embedding model for a chunk: its section heading (when the chunk does
/// not start with it) and the text.
pub fn embed_text(chunk_text: &str, heading: Option<&str>) -> String {
    match heading {
        Some(h) if !h.is_empty() && !chunk_text.contains(h.rsplit(" › ").next().unwrap_or(h)) => {
            format!("{h}\n\n{chunk_text}")
        }
        _ => chunk_text.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn one(text: &str) -> Vec<PageText> {
        vec![PageText {
            number: None,
            text: text.to_string(),
        }]
    }

    fn sentence(i: usize) -> String {
        format!("Cümle {i} burada biraz uzun bir açıklama içerir ve nokta ile biter.")
    }

    #[test]
    fn small_text_is_one_chunk() {
        let c = chunk_pages(&one("Merhaba.\n\nİkinci paragraf."));
        assert_eq!(c.len(), 1);
        assert_eq!(c[0].text, "Merhaba.\n\nİkinci paragraf.");
        assert_eq!(c[0].heading, None);
    }

    #[test]
    fn paragraphs_pack_to_target_with_overlap() {
        let paras: Vec<String> = (0..40)
            .map(|i| format!("{} {}", sentence(i), sentence(i + 100)))
            .collect();
        let c = chunk_pages(&one(&paras.join("\n\n")));
        assert!(c.len() > 3, "{}", c.len());
        for (i, ch) in c.iter().enumerate() {
            assert!(
                clen(&ch.text) <= TARGET_CHARS + OVERLAP_CHARS + 2,
                "chunk {i}: {}",
                clen(&ch.text)
            );
        }
        // Every paragraph is somewhere, and the next chunk starts with the previous tail.
        for p in &paras {
            assert!(c.iter().any(|ch| ch.text.contains(p.as_str())), "{p}");
        }
        for w in c.windows(2) {
            let t = tail(&w[0].text);
            assert!(!t.is_empty());
            assert!(w[1].text.starts_with(t), "{t:?} / {:?}", &w[1].text[..60]);
            assert!(clen(t) <= OVERLAP_CHARS);
        }
    }

    #[test]
    fn long_paragraph_splits_at_sentences() {
        let para: String = (0..60).map(sentence).collect::<Vec<_>>().join(" ");
        let pieces = split_long(&para);
        assert!(pieces.len() >= 3);
        for p in &pieces {
            assert!(clen(p) <= MAX_CHARS);
            assert!(p.ends_with('.'), "{p}");
        }
        assert_eq!(pieces.join(" "), para);
        // No sentence end at all: cut at spaces, never inside a word.
        let words = "kelime ".repeat(400);
        for p in split_long(&words) {
            assert!(clen(&p) <= MAX_CHARS);
            assert!(p.split(' ').all(|w| w == "kelime"));
        }
    }

    #[test]
    fn headings_start_chunks_and_are_remembered() {
        let body = |n: usize| (0..n).map(sentence).collect::<Vec<_>>().join(" ");
        let text = format!(
            "# Kılavuz\n\n{}\n\n## Kurulum\n{}\n\n## Kullanım\n\n{}\n\n# Ek\n\nkısa",
            body(4),
            body(5),
            body(3)
        );
        let c = chunk_pages(&one(&text));
        let headings: Vec<Option<&str>> = c.iter().map(|x| x.heading.as_deref()).collect();
        assert_eq!(
            headings,
            vec![
                Some("Kılavuz"),
                Some("Kılavuz › Kurulum"),
                Some("Kılavuz › Kullanım"),
                Some("Ek")
            ]
        );
        assert!(c[1].text.starts_with("## Kurulum\n\nCümle 0"));
        // No overlap across a section border.
        assert!(!c[2].text.contains("Kurulum"));
        // The heading is added for the embedding only when the chunk lacks it.
        assert_eq!(embed_text(&c[1].text, c[1].heading.as_deref()), c[1].text);
        assert_eq!(embed_text("gövde", Some("A › B")), "A › B\n\ngövde");
    }

    #[test]
    fn chunks_track_pages() {
        let pages: Vec<PageText> = (1..=3)
            .map(|n| PageText {
                number: Some(n),
                text: (0..8)
                    .map(|i| sentence(n as usize * 10 + i))
                    .collect::<Vec<_>>()
                    .join("\n\n"),
            })
            .collect();
        let c = chunk_pages(&pages);
        assert_eq!(c.first().unwrap().page_start, Some(1));
        assert_eq!(c.last().unwrap().page_end, Some(3));
        for ch in &c {
            assert!(ch.page_start <= ch.page_end);
        }
        let spans_two = c.iter().any(|ch| ch.page_start != ch.page_end);
        assert!(spans_two, "{c:#?}");
        // Sentence 21 is on page 2 and its chunk says so.
        let with = c.iter().find(|ch| ch.text.contains(&sentence(21))).unwrap();
        assert!(with.page_start.unwrap() <= 2 && with.page_end.unwrap() >= 2);
    }

    #[test]
    fn pdf_fixture_chunks() {
        let doc = super::super::extract::extract_file(
            &std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("tests/data/rag/state-db-schema.pdf"),
        )
        .unwrap();
        let c = chunk_pages(&doc.pages);
        assert!(c.len() >= 4, "{}", c.len());
        let v3 = c
            .iter()
            .find(|x| x.text.contains("vram_measurements"))
            .unwrap();
        assert_eq!(v3.page_start, Some(2));
        assert!(c
            .iter()
            .all(|x| clen(&x.text) <= TARGET_CHARS + OVERLAP_CHARS + MAX_CHARS));
    }
}
