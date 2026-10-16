//! Text tokenizers: the complete tokenizer.json, or the original BERT vocab.txt layout.

use std::path::Path;

use serde_json::Value;
use tokenizers::models::wordpiece::WordPiece;
use tokenizers::normalizers::bert::BertNormalizer;
use tokenizers::pre_tokenizers::bert::BertPreTokenizer;
use tokenizers::processors::bert::BertProcessing;
use tokenizers::{AddedToken, Tokenizer};

pub(crate) fn load(path: &Path) -> Result<Tokenizer, String> {
    let err = |e| format!("cannot read {}: {}", path.display(), e);
    if path.file_name().is_none_or(|n| n != "vocab.txt") {
        return Tokenizer::from_file(path).map_err(err);
    }
    let root = path.parent().ok_or("the vocabulary has no parent folder")?;
    let read = |name| {
        std::fs::read_to_string(root.join(name))
            .ok()
            .and_then(|s| serde_json::from_str::<Value>(s.trim_start_matches('\u{feff}')).ok())
            .unwrap_or(Value::Null)
    };
    let config = read("tokenizer_config.json");
    let special = read("special_tokens_map.json");
    let token = |name: &str, default: &str| {
        special
            .get(name)
            .or_else(|| config.get(name))
            .and_then(|v| {
                v.as_str()
                    .or_else(|| v.get("content").and_then(Value::as_str))
            })
            .unwrap_or(default)
            .to_string()
    };
    let unk = token("unk_token", "[UNK]");
    let model = WordPiece::from_file(path.to_str().ok_or("the vocabulary path is not UTF-8")?)
        .unk_token(unk.clone())
        .build()
        .map_err(err)?;
    let mut tokenizer = Tokenizer::new(model);
    let cls = token("cls_token", "[CLS]");
    let sep = token("sep_token", "[SEP]");
    let id = |name: &str| {
        tokenizer
            .token_to_id(name)
            .ok_or_else(|| format!("{} is missing special token {name}", path.display()))
    };
    let (cls_id, sep_id) = (id(&cls)?, id(&sep)?);
    id(&unk)?;
    tokenizer
        .with_normalizer(Some(BertNormalizer::new(
            true,
            config
                .get("tokenize_chinese_chars")
                .and_then(Value::as_bool)
                .unwrap_or(true),
            config.get("strip_accents").and_then(Value::as_bool),
            config
                .get("do_lower_case")
                .and_then(Value::as_bool)
                .unwrap_or(true),
        )))
        .map_err(|e| e.to_string())?;
    tokenizer.with_pre_tokenizer(Some(BertPreTokenizer));
    tokenizer.with_post_processor(Some(BertProcessing::new(
        (sep.clone(), sep_id),
        (cls.clone(), cls_id),
    )));
    let specials = [
        unk,
        cls,
        sep,
        token("pad_token", "[PAD]"),
        token("mask_token", "[MASK]"),
    ];
    let specials: Vec<AddedToken> = specials
        .into_iter()
        .filter(|t| tokenizer.token_to_id(t).is_some())
        .map(|t| AddedToken::from(t, true))
        .collect();
    tokenizer
        .add_special_tokens(specials)
        .map_err(|e| e.to_string())?;
    Ok(tokenizer)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn legacy_bert_preserves_case_offsets_pair_segments_and_mask_tokens() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("vocab.txt");
        std::fs::write(
            &path,
            "[PAD]\n[UNK]\n[CLS]\n[SEP]\n[MASK]\nJohn\njohn\nSmith\nworks\n.\n",
        )
        .unwrap();
        std::fs::write(
            dir.path().join("tokenizer_config.json"),
            r#"{"do_lower_case": false}"#,
        )
        .unwrap();
        let tokenizer = load(&path).unwrap();
        let one = tokenizer.encode("John Smith works.", true).unwrap();
        assert_eq!(
            one.get_tokens(),
            &["[CLS]", "John", "Smith", "works", ".", "[SEP]"]
        );
        assert_eq!(one.get_offsets()[1], (0, 4));
        assert_eq!(one.get_offsets()[2], (5, 10));
        let pair = tokenizer.encode(("John", "Smith"), true).unwrap();
        assert_eq!(pair.get_ids(), &[2, 5, 3, 7, 3]);
        assert_eq!(pair.get_type_ids(), &[0, 0, 0, 1, 1]);
        assert_eq!(
            pair.get_sequence_ids(),
            vec![None, Some(0), None, Some(1), None]
        );
        assert!(tokenizer
            .encode("John [MASK]", true)
            .unwrap()
            .get_ids()
            .contains(&4));
        std::fs::write(
            dir.path().join("tokenizer_config.json"),
            r#"{"do_lower_case": true}"#,
        )
        .unwrap();
        assert_eq!(
            load(&path).unwrap().encode("John", true).unwrap().get_ids(),
            &[2, 6, 3]
        );
    }

    fn wordpiece(vocab: &[&str], config: &str) -> (tempfile::TempDir, Tokenizer) {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("vocab.txt");
        let mut text = String::from("[PAD]\n[UNK]\n[CLS]\n[SEP]\n[MASK]\n");
        for t in vocab {
            text.push_str(t);
            text.push('\n');
        }
        std::fs::write(&path, text).unwrap();
        std::fs::write(dir.path().join("tokenizer_config.json"), config).unwrap();
        let tokenizer = load(&path).unwrap();
        (dir, tokenizer)
    }

    fn tokens(tokenizer: &Tokenizer, text: &str) -> Vec<String> {
        tokenizer
            .encode(text, true)
            .unwrap()
            .get_tokens()
            .iter()
            .map(|t| t.to_string())
            .collect()
    }

    #[test]
    fn wordpiece_accents_follow_strip_accents_and_lowercase() {
        let vocab = ["cafe", "Cafe", "caf\u{e9}", "Caf\u{e9}"];
        // Lowercase on and strip_accents unset: the accents go with the case.
        let (_d, t) = wordpiece(&vocab, r#"{"do_lower_case": true}"#);
        assert_eq!(tokens(&t, "Caf\u{e9}"), ["[CLS]", "cafe", "[SEP]"]);
        // Lowercase off and strip_accents unset: both are kept.
        let (_d, t) = wordpiece(&vocab, r#"{"do_lower_case": false}"#);
        assert_eq!(tokens(&t, "Caf\u{e9}"), ["[CLS]", "Caf\u{e9}", "[SEP]"]);
        assert_eq!(tokens(&t, "Cafe"), ["[CLS]", "Cafe", "[SEP]"]);
        // An explicit strip_accents wins over the case flag, both ways.
        let (_d, t) = wordpiece(&vocab, r#"{"do_lower_case": false, "strip_accents": true}"#);
        assert_eq!(tokens(&t, "Caf\u{e9}"), ["[CLS]", "Cafe", "[SEP]"]);
        let (_d, t) = wordpiece(&vocab, r#"{"do_lower_case": true, "strip_accents": false}"#);
        assert_eq!(tokens(&t, "Caf\u{e9}"), ["[CLS]", "caf\u{e9}", "[SEP]"]);
    }

    #[test]
    fn wordpiece_unknown_and_overlong_words_are_unk() {
        let (_d, t) = wordpiece(&["play", "a", "##a", "##ing"], r#"{"do_lower_case": true}"#);
        // A piece the vocabulary lacks makes the whole word [UNK].
        assert_eq!(tokens(&t, "zzz"), ["[CLS]", "[UNK]", "[SEP]"]);
        assert_eq!(tokens(&t, "playx"), ["[CLS]", "[UNK]", "[SEP]"]);
        assert_eq!(tokens(&t, "playing"), ["[CLS]", "play", "##ing", "[SEP]"]);
        assert_eq!(
            tokens(&t, "playing zzz play"),
            ["[CLS]", "play", "##ing", "[UNK]", "play", "[SEP]"]
        );
        // max_input_chars_per_word is 100: 100 characters still split, 101 are [UNK].
        let split = tokens(&t, &"a".repeat(100));
        assert_eq!(split.len(), 102);
        assert_eq!(split[1], "a");
        assert_eq!(split[2], "##a");
        assert_eq!(tokens(&t, &"a".repeat(101)), ["[CLS]", "[UNK]", "[SEP]"]);
        // The overlong word does not swallow its neighbours.
        assert_eq!(
            tokens(&t, &format!("play {} play", "a".repeat(101))),
            ["[CLS]", "play", "[UNK]", "play", "[SEP]"]
        );
    }
}
