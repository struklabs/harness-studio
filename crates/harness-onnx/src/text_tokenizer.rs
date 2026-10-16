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
}
