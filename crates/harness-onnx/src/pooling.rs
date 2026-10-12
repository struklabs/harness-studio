//! Pooling, normalization and score maths on the model's raw output (pure functions).
//!
//! Token vectors come out of the graph as `[batch, seq, dim]` (row-major, padded to the longest
//! text of the batch); `lengths[b]` is the number of real tokens of text `b` (the attention mask
//! of right-padded input). A pooled vector never sees padding.

use crate::package::Pooling;

/// Pools `hidden` (`batch * seq * dim` values) into one vector per text.
pub fn pool(
    mode: Pooling,
    hidden: &[f32],
    batch: usize,
    seq: usize,
    dim: usize,
    lengths: &[usize],
) -> Vec<Vec<f32>> {
    debug_assert_eq!(hidden.len(), batch * seq * dim);
    debug_assert_eq!(lengths.len(), batch);
    (0..batch)
        .map(|b| {
            let n = lengths[b].clamp(1, seq.max(1));
            let row = &hidden[b * seq * dim..(b + 1) * seq * dim];
            let token = |t: usize| &row[t * dim..(t + 1) * dim];
            match mode {
                Pooling::Cls => token(0).to_vec(),
                Pooling::LastToken => token(n - 1).to_vec(),
                Pooling::Max => {
                    let mut out = token(0).to_vec();
                    for t in 1..n {
                        for (o, v) in out.iter_mut().zip(token(t)) {
                            if *v > *o {
                                *o = *v;
                            }
                        }
                    }
                    out
                }
                Pooling::Mean | Pooling::MeanSqrtLen | Pooling::WeightedMean => {
                    let mut out = vec![0f32; dim];
                    let mut weight_sum = 0f32;
                    for t in 0..n {
                        let w = if mode == Pooling::WeightedMean {
                            (t + 1) as f32
                        } else {
                            1.0
                        };
                        weight_sum += w;
                        for (o, v) in out.iter_mut().zip(token(t)) {
                            *o += w * v;
                        }
                    }
                    let div = match mode {
                        Pooling::MeanSqrtLen => (n as f32).sqrt(),
                        _ => weight_sum,
                    };
                    for o in &mut out {
                        *o /= div;
                    }
                    out
                }
            }
        })
        .collect()
}

/// Scales `v` to unit length (a zero vector stays zero).
pub fn l2_normalize(v: &mut [f32]) {
    let norm = v.iter().map(|x| x * x).sum::<f32>().sqrt();
    if norm > 0.0 {
        for x in v {
            *x /= norm;
        }
    }
}

/// Keeps the first `dims` values and renormalizes (Matryoshka-style `dimensions`).
pub fn truncate_dims(v: &mut Vec<f32>, dims: usize, normalize: bool) {
    if dims > 0 && dims < v.len() {
        v.truncate(dims);
        if normalize {
            l2_normalize(v);
        }
    }
}

/// Cosine similarity of two vectors of equal length.
pub fn cosine(a: &[f32], b: &[f32]) -> f32 {
    let dot: f32 = a.iter().zip(b).map(|(x, y)| x * y).sum();
    let na = a.iter().map(|x| x * x).sum::<f32>().sqrt();
    let nb = b.iter().map(|x| x * x).sum::<f32>().sqrt();
    if na == 0.0 || nb == 0.0 {
        0.0
    } else {
        dot / (na * nb)
    }
}

/// Relevance score of a cross-encoder row: the single logit, or for a model with several
/// labels the last one (the "relevant" class of a two-label head, as bge/mixedbread rerankers
/// and `AutoModelForSequenceClassification` pipelines read it).
pub fn rerank_score(logits: &[f32]) -> f32 {
    logits.last().copied().unwrap_or(0.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Two texts, seq 3, dim 2: text 0 has 2 real tokens, text 1 has 3.
    fn hidden() -> Vec<f32> {
        vec![
            // text 0: t0, t1, padding (garbage that must not count)
            1.0, 2.0, 3.0, 4.0, 99.0, 99.0, //
            // text 1
            -1.0, 0.0, 5.0, 2.0, 1.0, 8.0,
        ]
    }

    #[test]
    fn mean_ignores_padding() {
        let p = pool(Pooling::Mean, &hidden(), 2, 3, 2, &[2, 3]);
        assert_eq!(p[0], vec![2.0, 3.0]);
        assert_eq!(p[1], vec![5.0 / 3.0, 10.0 / 3.0]);
    }

    #[test]
    fn cls_last_and_max() {
        let h = hidden();
        assert_eq!(pool(Pooling::Cls, &h, 2, 3, 2, &[2, 3])[1], vec![-1.0, 0.0]);
        let last = pool(Pooling::LastToken, &h, 2, 3, 2, &[2, 3]);
        assert_eq!(last[0], vec![3.0, 4.0]);
        assert_eq!(last[1], vec![1.0, 8.0]);
        let max = pool(Pooling::Max, &h, 2, 3, 2, &[2, 3]);
        assert_eq!(max[0], vec![3.0, 4.0]);
        assert_eq!(max[1], vec![5.0, 8.0]);
    }

    #[test]
    fn sqrt_len_and_weighted_mean() {
        let h = hidden();
        let s = pool(Pooling::MeanSqrtLen, &h, 2, 3, 2, &[2, 3]);
        let r = 2f32.sqrt();
        assert!((s[0][0] - 4.0 / r).abs() < 1e-6 && (s[0][1] - 6.0 / r).abs() < 1e-6);
        let w = pool(Pooling::WeightedMean, &h, 2, 3, 2, &[2, 3]);
        // text 0: (1*[1,2] + 2*[3,4]) / 3
        assert!((w[0][0] - 7.0 / 3.0).abs() < 1e-6 && (w[0][1] - 10.0 / 3.0).abs() < 1e-6);
    }

    #[test]
    fn normalize_and_cosine() {
        let mut v = vec![3.0, 4.0];
        l2_normalize(&mut v);
        assert!((v[0] - 0.6).abs() < 1e-6 && (v[1] - 0.8).abs() < 1e-6);
        let mut z = vec![0.0, 0.0];
        l2_normalize(&mut z);
        assert_eq!(z, vec![0.0, 0.0]);
        assert!((cosine(&[1.0, 0.0], &[1.0, 0.0]) - 1.0).abs() < 1e-6);
        assert!(cosine(&[1.0, 0.0], &[0.0, 1.0]).abs() < 1e-6);
        assert_eq!(cosine(&[0.0], &[1.0]), 0.0);
    }

    #[test]
    fn matryoshka_truncation_renormalizes() {
        let mut v = vec![0.5, 0.5, 0.5, 0.5];
        truncate_dims(&mut v, 2, true);
        assert_eq!(v.len(), 2);
        assert!((v[0] - std::f32::consts::FRAC_1_SQRT_2).abs() < 1e-6);
        let mut same = vec![1.0, 2.0];
        truncate_dims(&mut same, 8, true);
        assert_eq!(same, vec![1.0, 2.0]);
    }

    #[test]
    fn rerank_reads_the_relevant_label() {
        assert_eq!(rerank_score(&[2.5]), 2.5);
        assert_eq!(rerank_score(&[-1.0, 3.0]), 3.0);
        assert_eq!(rerank_score(&[]), 0.0);
    }
}
