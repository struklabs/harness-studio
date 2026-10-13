//! What the encoder task heads' logits mean, without a session: label scores of a classifier,
//! zero-shot scores of an NLI head, entities from per-token labels, mask fills and answer spans.
//! Follows the post-processing of the Hugging Face `transformers` pipelines of the same names.
//! Pure.

/// Softmax of one row of logits.
pub fn softmax(row: &[f32]) -> Vec<f32> {
    let max = row.iter().copied().fold(f32::NEG_INFINITY, f32::max);
    let exps: Vec<f32> = row.iter().map(|v| (v - max).exp()).collect();
    let sum: f32 = exps.iter().sum();
    exps.into_iter().map(|e| e / sum).collect()
}

pub fn sigmoid(v: f32) -> f32 {
    1.0 / (1.0 + (-v).exp())
}

/// A label and its score.
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct LabelScore {
    pub label: String,
    pub score: f32,
}

/// Name of label `i`: `labels[i]`, else `LABEL_i` (what transformers calls an unnamed one).
pub fn label_name(labels: &[String], i: usize) -> String {
    labels
        .get(i)
        .cloned()
        .unwrap_or_else(|| format!("LABEL_{i}"))
}

/// Label scores of one row of classifier logits, best first: softmax (or a sigmoid per label
/// when the labels are independent, or when the head has one output), `top_k` of them.
pub fn classify_row(
    row: &[f32],
    labels: &[String],
    multi_label: bool,
    top_k: Option<usize>,
) -> Vec<LabelScore> {
    let scores = if multi_label || row.len() == 1 {
        row.iter().map(|v| sigmoid(*v)).collect()
    } else {
        softmax(row)
    };
    let mut out: Vec<LabelScore> = scores
        .into_iter()
        .enumerate()
        .map(|(i, score)| LabelScore {
            label: label_name(labels, i),
            score,
        })
        .collect();
    out.sort_by(|a, b| b.score.total_cmp(&a.score));
    if let Some(k) = top_k {
        out.truncate(k.max(1));
    }
    out
}

/// Index of the `entailment` and `contradiction` labels of an NLI head (`None` when the head
/// has no entailment label: not usable for zero-shot classification).
pub fn nli_indices(labels: &[String]) -> Option<(usize, Option<usize>)> {
    let find = |name: &str| {
        labels
            .iter()
            .position(|l| l.to_ascii_lowercase().starts_with(name))
    };
    Some((find("entail")?, find("contra")))
}

/// Zero-shot scores of the candidate labels from one NLI logit row per candidate
/// (premise = the text, hypothesis = the template filled with the label). Single-label: a
/// softmax of the entailment logits across candidates (they sum to 1). Multi-label: per
/// candidate, a softmax of entailment against contradiction. Best first.
pub fn zero_shot(
    rows: &[Vec<f32>],
    candidates: &[String],
    entail: usize,
    contra: Option<usize>,
    multi_label: bool,
) -> Vec<LabelScore> {
    let scores: Vec<f32> = if multi_label || candidates.len() == 1 {
        rows.iter()
            .map(|r| {
                let e = r.get(entail).copied().unwrap_or(0.0);
                match contra.and_then(|c| r.get(c)) {
                    Some(c) => softmax(&[*c, e])[1],
                    None => sigmoid(e),
                }
            })
            .collect()
    } else {
        let entails: Vec<f32> = rows
            .iter()
            .map(|r| r.get(entail).copied().unwrap_or(0.0))
            .collect();
        softmax(&entails)
    };
    let mut out: Vec<LabelScore> = candidates
        .iter()
        .cloned()
        .zip(scores)
        .map(|(label, score)| LabelScore { label, score })
        .collect();
    out.sort_by(|a, b| b.score.total_cmp(&a.score));
    out
}

/// One token of an encoded text, for entity grouping.
#[derive(Debug, Clone)]
pub struct TokenInfo {
    /// Byte offsets of the token in the text.
    pub start: usize,
    pub end: usize,
    /// A special token (`[CLS]`, `[SEP]`, padding): never part of an entity.
    pub special: bool,
    /// Index of the word the token belongs to (`None` for special tokens).
    pub word: Option<u32>,
}

/// An entity (or a single token when not grouped).
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct Entity {
    /// Grouped: the label without its `B-` / `I-` prefix (`entity_group`); single tokens keep
    /// the full label.
    pub entity: String,
    pub score: f32,
    pub word: String,
    pub start: usize,
    pub end: usize,
}

/// `B-PER` -> (`B`, `PER`); `PER` -> (`I`, `PER`) (no prefix counts as inside).
fn split_tag(label: &str) -> (char, &str) {
    match label.split_once('-') {
        Some((p, rest)) if p.len() == 1 && "BIESL".contains(p) => {
            (p.chars().next().unwrap_or('I'), rest)
        }
        _ => ('I', label),
    }
}

/// An entity being built: type, token scores, start and end offsets, word index.
type Group = (String, Vec<f32>, usize, usize, Option<u32>);

/// Entities of one text from its per-token logits (`[seq, labels]` row-major). `group`: merge
/// the tokens of an entity (`aggregation_strategy="simple"`: consecutive tokens with the same
/// type, a `B-` starting a new one, the score the mean); otherwise one entry per token. The `O`
/// label (outside) is dropped either way.
pub fn token_entities(
    text: &str,
    logits: &[f32],
    num_labels: usize,
    tokens: &[TokenInfo],
    labels: &[String],
    group: bool,
) -> Vec<Entity> {
    struct Tagged {
        label: String,
        score: f32,
        start: usize,
        end: usize,
        word: Option<u32>,
    }
    let mut tagged: Vec<Tagged> = Vec::new();
    for (t, tok) in tokens.iter().enumerate() {
        if tok.special || tok.end <= tok.start {
            continue;
        }
        let Some(row) = logits.get(t * num_labels..(t + 1) * num_labels) else {
            break;
        };
        let probs = softmax(row);
        let (best, score) = probs
            .iter()
            .enumerate()
            .max_by(|a, b| a.1.total_cmp(b.1))
            .map(|(i, s)| (i, *s))
            .unwrap_or((0, 0.0));
        tagged.push(Tagged {
            label: label_name(labels, best),
            score,
            start: tok.start,
            end: tok.end,
            word: tok.word,
        });
    }
    let slice = |s: usize, e: usize| text.get(s..e).unwrap_or("").to_string();
    if !group {
        return tagged
            .into_iter()
            .filter(|t| t.label != "O")
            .map(|t| Entity {
                word: slice(t.start, t.end),
                entity: t.label,
                score: t.score,
                start: t.start,
                end: t.end,
            })
            .collect();
    }
    let mut out: Vec<Entity> = Vec::new();
    let mut current: Option<Group> = None;
    let flush = |cur: &mut Option<Group>, out: &mut Vec<Entity>| {
        if let Some((kind, scores, start, end, _)) = cur.take() {
            out.push(Entity {
                score: scores.iter().sum::<f32>() / scores.len().max(1) as f32,
                word: slice(start, end).trim().to_string(),
                entity: kind,
                start,
                end,
            });
        }
    };
    for t in tagged {
        if t.label == "O" {
            flush(&mut current, &mut out);
            continue;
        }
        let (prefix, kind) = split_tag(&t.label);
        let continues = match &current {
            // A sub-word of the same word always continues; otherwise the type must match and
            // the tag must not start a new entity.
            Some((k, _, _, _, w)) => {
                (t.word.is_some() && *w == t.word) || (k == kind && prefix != 'B')
            }
            None => false,
        };
        if continues {
            if let Some((_, scores, _, end, w)) = current.as_mut() {
                scores.push(t.score);
                *end = t.end;
                *w = t.word;
            }
        } else {
            flush(&mut current, &mut out);
            current = Some((kind.to_string(), vec![t.score], t.start, t.end, t.word));
        }
    }
    flush(&mut current, &mut out);
    out
}

/// The `k` most likely token ids of one row of vocabulary logits, with probabilities.
pub fn top_tokens(row: &[f32], k: usize) -> Vec<(u32, f32)> {
    let probs = softmax(row);
    let mut idx: Vec<(u32, f32)> = probs
        .into_iter()
        .enumerate()
        .map(|(i, p)| (i as u32, p))
        .collect();
    let k = k.max(1).min(idx.len());
    if k == 0 {
        return Vec::new();
    }
    idx.select_nth_unstable_by(k - 1, |a, b| b.1.total_cmp(&a.1));
    idx.truncate(k);
    idx.sort_by(|a, b| b.1.total_cmp(&a.1));
    idx
}

/// An answer span.
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct Answer {
    pub answer: String,
    pub score: f32,
    pub start: usize,
    pub end: usize,
}

/// The best answer spans of one (question, context) encoding: `start` / `end` logits per token,
/// `context_tokens` the token indices that belong to the context (with their byte offsets).
/// A span runs from a start token to an end token at or after it, at most `max_answer_tokens`
/// long; its score is p(start) * p(end), softmax over the context tokens (like the
/// transformers pipeline). Best first, `top_k` of them.
pub fn answer_spans(
    context: &str,
    start_logits: &[f32],
    end_logits: &[f32],
    context_tokens: &[(usize, usize, usize)],
    max_answer_tokens: usize,
    top_k: usize,
) -> Vec<Answer> {
    if context_tokens.is_empty() {
        return Vec::new();
    }
    let pick = |logits: &[f32]| -> Vec<f32> {
        let row: Vec<f32> = context_tokens
            .iter()
            .map(|(t, _, _)| logits.get(*t).copied().unwrap_or(f32::NEG_INFINITY))
            .collect();
        softmax(&row)
    };
    let ps = pick(start_logits);
    let pe = pick(end_logits);
    let mut spans: Vec<(f32, usize, usize)> = Vec::new();
    let longest = max_answer_tokens.max(1);
    for (i, s) in ps.iter().enumerate() {
        for (j, e) in pe.iter().enumerate().skip(i).take(longest) {
            spans.push((s * e, i, j));
        }
    }
    spans.sort_by(|a, b| b.0.total_cmp(&a.0));
    let mut out: Vec<Answer> = Vec::new();
    for (score, i, j) in spans {
        let start = context_tokens[i].1;
        let end = context_tokens[j].2;
        if end <= start || out.iter().any(|a| a.start == start && a.end == end) {
            continue;
        }
        out.push(Answer {
            answer: context.get(start..end).unwrap_or("").to_string(),
            score,
            start,
            end,
        });
        if out.len() >= top_k.max(1) {
            break;
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(v: &[&str]) -> Vec<String> {
        v.iter().map(|x| x.to_string()).collect()
    }

    #[test]
    fn classifier_scores_are_a_softmax_best_first() {
        let r = classify_row(&[0.0, 2.0], &s(&["NEGATIVE", "POSITIVE"]), false, None);
        assert_eq!(r[0].label, "POSITIVE");
        assert!((r[0].score + r[1].score - 1.0).abs() < 1e-6);
        assert!((r[0].score - 0.880797).abs() < 1e-5);
        // Independent labels: a sigmoid each; unnamed labels get LABEL_i; top_k cuts.
        let m = classify_row(&[0.0, 0.0, 3.0], &[], true, Some(2));
        assert_eq!(m.len(), 2);
        assert_eq!(m[0].label, "LABEL_2");
        assert!((m[1].score - 0.5).abs() < 1e-6);
    }

    #[test]
    fn zero_shot_single_and_multi_label() {
        let labels = s(&["contradiction", "neutral", "entailment"]);
        let (e, c) = nli_indices(&labels).unwrap();
        assert_eq!((e, c), (2, Some(0)));
        assert!(nli_indices(&s(&["NEGATIVE", "POSITIVE"])).is_none());
        let rows = vec![vec![-2.0, 0.0, 3.0], vec![2.0, 0.0, -1.0]];
        let cands = s(&["sports", "politics"]);
        let single = zero_shot(&rows, &cands, e, c, false);
        assert_eq!(single[0].label, "sports");
        assert!((single[0].score + single[1].score - 1.0).abs() < 1e-6);
        let multi = zero_shot(&rows, &cands, e, c, true);
        // sports: softmax(-2, 3)[1]; politics: softmax(2, -1)[1].
        assert!((multi[0].score - 0.993307).abs() < 1e-5);
        assert!((multi[1].score - 0.047426).abs() < 1e-5);
    }

    #[test]
    fn tokens_are_grouped_into_entities() {
        // "Wolfgang lives in Berlin": [CLS] Wolf ##gang lives in Berlin [SEP]
        let text = "Wolfgang lives in Berlin";
        let labels = s(&["O", "B-PER", "I-PER", "B-LOC", "I-LOC"]);
        let tok = |start, end, word| TokenInfo {
            start,
            end,
            special: false,
            word: Some(word),
        };
        let special = TokenInfo {
            start: 0,
            end: 0,
            special: true,
            word: None,
        };
        let tokens = vec![
            special.clone(),
            tok(0, 4, 0),
            tok(4, 8, 0),
            tok(9, 14, 1),
            tok(15, 17, 2),
            tok(18, 24, 3),
            special,
        ];
        let hot = |i: usize| {
            let mut r = vec![0.0f32; 5];
            r[i] = 5.0;
            r
        };
        let logits: Vec<f32> = [0, 1, 2, 0, 0, 3, 0].iter().flat_map(|&i| hot(i)).collect();
        let grouped = token_entities(text, &logits, 5, &tokens, &labels, true);
        assert_eq!(grouped.len(), 2);
        assert_eq!(
            (grouped[0].entity.as_str(), grouped[0].word.as_str()),
            ("PER", "Wolfgang")
        );
        assert_eq!(
            (
                grouped[1].entity.as_str(),
                grouped[1].word.as_str(),
                grouped[1].start
            ),
            ("LOC", "Berlin", 18)
        );
        let flat = token_entities(text, &logits, 5, &tokens, &labels, false);
        assert_eq!(flat.len(), 3);
        assert_eq!(flat[1].entity, "I-PER");
        assert_eq!(flat[1].word, "gang");
    }

    #[test]
    fn mask_fills_and_answer_spans() {
        let top = top_tokens(&[0.0, 3.0, 1.0, 2.0], 2);
        assert_eq!(top.iter().map(|t| t.0).collect::<Vec<_>>(), vec![1, 3]);
        // context "in Paris today": tokens 3,4,5 of the pair encoding.
        let ctx = "in Paris today";
        let toks = vec![(3, 0, 2), (4, 3, 8), (5, 9, 14)];
        let mut start = vec![0.0; 7];
        let mut end = vec![0.0; 7];
        start[4] = 6.0;
        end[4] = 6.0;
        let a = answer_spans(ctx, &start, &end, &toks, 15, 2);
        assert_eq!(a[0].answer, "Paris");
        assert_eq!((a[0].start, a[0].end), (3, 8));
        assert_eq!(a.len(), 2);
        assert!(a[0].score > a[1].score);
    }
}
