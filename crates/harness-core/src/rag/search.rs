//! Retrieval math: vectors, exact (brute-force) nearest neighbours, keyword queries and rank
//! fusion.
//!
//! Vectors are L2-normalized before they are stored, so cosine similarity is a dot product.
//! Search is exact over every stored vector of the conversation: with the per-conversation
//! cap ([`super::MAX_CHUNKS_PER_CONVERSATION`] chunks × e.g. 768 dims) that is at most a
//! few million multiply-adds, a few milliseconds, and needs no index structure.
//!
//! Dense and keyword (SQLite FTS5 BM25) rankings are merged with Reciprocal Rank Fusion
//! (`1 / (60 + rank)`): exact terms (identifiers, names, numbers) that embeddings blur are
//! still found, and without an embedding model keyword search works alone.

/// RRF constant (Cormack et al. 2009).
pub const RRF_K: f32 = 60.0;

/// Scales `v` to unit length (a zero vector stays zero).
pub fn normalize(v: &mut [f32]) {
    let norm = v.iter().map(|x| x * x).sum::<f32>().sqrt();
    if norm > 0.0 {
        for x in v.iter_mut() {
            *x /= norm;
        }
    }
}

pub fn dot(a: &[f32], b: &[f32]) -> f32 {
    a.iter().zip(b).map(|(x, y)| x * y).sum()
}

/// Cosine similarity (for vectors that may not be normalized).
pub fn cosine(a: &[f32], b: &[f32]) -> f32 {
    let (na, nb) = (dot(a, a).sqrt(), dot(b, b).sqrt());
    if na == 0.0 || nb == 0.0 {
        0.0
    } else {
        dot(a, b) / (na * nb)
    }
}

/// Little-endian f32 bytes (the BLOB format of `rag_chunks.embedding`).
pub fn to_blob(v: &[f32]) -> Vec<u8> {
    v.iter().flat_map(|x| x.to_le_bytes()).collect()
}

pub fn from_blob(b: &[u8]) -> Vec<f32> {
    b.as_chunks::<4>()
        .0
        .iter()
        .map(|c| f32::from_le_bytes(*c))
        .collect()
}

/// The `k` best `(id, score)` by descending score (ties keep input order).
pub fn top_k<T: Clone>(scored: &[(T, f32)], k: usize) -> Vec<(T, f32)> {
    let mut idx: Vec<usize> = (0..scored.len()).collect();
    idx.sort_by(|&a, &b| {
        scored[b]
            .1
            .partial_cmp(&scored[a].1)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then(a.cmp(&b))
    });
    idx.into_iter().take(k).map(|i| scored[i].clone()).collect()
}

/// Exact nearest neighbours of `query` among `(id, unit vector)`; vectors of another
/// dimension are skipped.
pub fn nearest<T: Clone>(query: &[f32], vectors: &[(T, Vec<f32>)], k: usize) -> Vec<(T, f32)> {
    let scored: Vec<(T, f32)> = vectors
        .iter()
        .filter(|(_, v)| v.len() == query.len())
        .map(|(id, v)| (id.clone(), dot(query, v)))
        .collect();
    top_k(&scored, k)
}

/// Adds the similarity of every id in `ids` that has none yet (a chunk found only by the
/// keyword ranking is outside the dense top list). Uses the same score as [`nearest`]; ids
/// without a vector of the query's dimension stay unset.
pub fn fill_similarity(
    similarity: &mut std::collections::HashMap<i64, f32>,
    query: &[f32],
    vectors: &[(i64, Vec<f32>)],
    ids: &[i64],
) {
    for (id, v) in vectors {
        if v.len() == query.len() && ids.contains(id) && !similarity.contains_key(id) {
            similarity.insert(*id, dot(query, v));
        }
    }
}

/// Reciprocal Rank Fusion of several rankings (best first) → `(id, fused score)`, best first.
pub fn rrf<T: Clone + Eq + std::hash::Hash>(rankings: &[Vec<T>]) -> Vec<(T, f32)> {
    let mut scores: Vec<(T, f32)> = Vec::new();
    let mut pos: std::collections::HashMap<T, usize> = std::collections::HashMap::new();
    for ranking in rankings {
        for (rank, id) in ranking.iter().enumerate() {
            let add = 1.0 / (RRF_K + rank as f32 + 1.0);
            match pos.get(id) {
                Some(&i) => scores[i].1 += add,
                None => {
                    pos.insert(id.clone(), scores.len());
                    scores.push((id.clone(), add));
                }
            }
        }
    }
    top_k(&scores, scores.len())
}

/// FTS5 query for a question: its words (letters, digits, `_`; at least 2 characters,
/// at most 32 distinct), each quoted, joined by OR. `None` when nothing is left.
pub fn fts_query(text: &str) -> Option<String> {
    let mut words: Vec<String> = Vec::new();
    for w in text.split(|c: char| !(c.is_alphanumeric() || c == '_')) {
        let w = w.to_lowercase();
        if w.chars().count() >= 2 && !words.contains(&w) {
            words.push(w);
        }
        if words.len() == 32 {
            break;
        }
    }
    (!words.is_empty()).then(|| {
        words
            .iter()
            .map(|w| format!("\"{w}\""))
            .collect::<Vec<_>>()
            .join(" OR ")
    })
}

/// Rough token count of a text (≈ 3 characters per token; errs on the high side for
/// prose so "fits in the context" is conservative).
pub fn estimate_tokens(chars: usize) -> usize {
    chars.div_ceil(3)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_and_dot() {
        let mut v = vec![3.0, 4.0];
        normalize(&mut v);
        assert!((v[0] - 0.6).abs() < 1e-6 && (v[1] - 0.8).abs() < 1e-6);
        let mut z = vec![0.0, 0.0];
        normalize(&mut z);
        assert_eq!(z, vec![0.0, 0.0]);
        assert!((cosine(&[1.0, 0.0], &[2.0, 0.0]) - 1.0).abs() < 1e-6);
        assert!(cosine(&[1.0, 0.0], &[0.0, 5.0]).abs() < 1e-6);
        assert!((cosine(&[1.0, 1.0], &[-1.0, -1.0]) + 1.0).abs() < 1e-6);
        assert_eq!(cosine(&[0.0], &[1.0]), 0.0);
    }

    #[test]
    fn blob_round_trip() {
        let v = vec![0.5f32, -1.25, 3.0e-8, f32::MAX];
        let b = to_blob(&v);
        assert_eq!(b.len(), 16);
        assert_eq!(from_blob(&b), v);
    }

    #[test]
    fn nearest_is_exact_and_skips_other_dims() {
        let mut vecs: Vec<(u32, Vec<f32>)> = Vec::new();
        for i in 0..100u32 {
            let a = i as f32 * 0.05;
            vecs.push((i, vec![a.cos(), a.sin()]));
        }
        vecs.push((999, vec![1.0, 0.0, 0.0]));
        let mut q = vec![(1.01f32).cos(), (1.01f32).sin()];
        normalize(&mut q);
        let got = nearest(&q, &vecs, 3);
        let ids: Vec<u32> = got.iter().map(|(id, _)| *id).collect();
        assert_eq!(ids, vec![20, 21, 19]);
        assert!((got[0].1 - 1.0).abs() < 1e-3);
        assert!(got[1].1 >= got[2].1);
    }

    #[test]
    fn fill_similarity_covers_ids_outside_the_dense_list() {
        let vecs: Vec<(i64, Vec<f32>)> = vec![
            (1, vec![1.0, 0.0]),
            (2, vec![0.6, 0.8]),
            (3, vec![1.0, 0.0, 0.0]),
        ];
        let q = vec![1.0f32, 0.0];
        // Only chunk 1 was in the dense top list; chunk 2 came in through keywords only.
        let mut sim: std::collections::HashMap<i64, f32> =
            nearest(&q, &vecs[..1], 1).into_iter().collect();
        sim.insert(1, 0.5);
        fill_similarity(&mut sim, &q, &vecs, &[1, 2, 3, 4]);
        assert_eq!(sim.get(&1), Some(&0.5), "an existing value is kept");
        assert!((sim[&2] - 0.6).abs() < 1e-6);
        assert_eq!(sim.get(&3), None, "other dimension");
        assert_eq!(sim.get(&4), None, "no vector");
    }

    #[test]
    fn rrf_rewards_agreement() {
        let dense = vec!["a", "b", "c", "d"];
        let lexical = vec!["c", "e", "a"];
        let fused = rrf(&[dense, lexical]);
        let order: Vec<&str> = fused.iter().map(|(id, _)| *id).collect();
        // a: 1/61 + 1/63, c: 1/63 + 1/61 → tie, a came first; then b (1/62), e (1/62), d.
        assert_eq!(order, vec!["a", "c", "b", "e", "d"]);
        assert!((fused[0].1 - (1.0 / 61.0 + 1.0 / 63.0)).abs() < 1e-7);
    }

    #[test]
    fn fts_query_quotes_words() {
        assert_eq!(
            fts_query("v3 migration'ı hangi tabloyu ekler? vram_measurements!").as_deref(),
            Some("\"v3\" OR \"migration\" OR \"hangi\" OR \"tabloyu\" OR \"ekler\" OR \"vram_measurements\"")
        );
        assert_eq!(fts_query("a ? !"), None);
        // Quotes and FTS operators in the question cannot break the query.
        assert_eq!(
            fts_query("\"NEAR\" x* AND y").as_deref(),
            Some("\"near\" OR \"and\"")
        );
    }

    #[test]
    fn token_estimate_is_conservative() {
        assert_eq!(estimate_tokens(0), 0);
        assert_eq!(estimate_tokens(10), 4);
        assert_eq!(estimate_tokens(3000), 1000);
    }
}
