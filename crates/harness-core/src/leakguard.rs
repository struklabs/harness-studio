//! Credential-leak guard: finds API keys and tokens in text that is about to leave the machine
//! or be written to a log, and replaces them with a marker.
//!
//! Two strengths, because the cost of a false positive differs:
//!
//! * [`Mode::Tokens`] only matches well-known, self-identifying formats (OpenAI/Anthropic
//!   `sk-...`, Hugging Face `hf_...`, GitHub, AWS, Google, Slack tokens, JWTs, PEM private
//!   keys, `Bearer` credentials). It is what outgoing payloads get (BYOK cloud requests, tool
//!   results sent to a model): a prompt that talks about `password = ...` is left alone.
//! * [`Mode::Broad`] adds `name = value` assignments of secret-looking names. It is what logs
//!   and the tool log get, where over-redacting costs nothing.
//!
//! The guard is best effort: it removes the credentials that look like credentials. It does
//! not make a leak impossible (a key a user pasted in a different shape stays), which
//! `docs/guide/en/security.md` says plainly.

use std::sync::OnceLock;

use regex::Regex;
use serde_json::Value;

/// What a found secret is replaced with.
pub const MARKER: &str = "[REDACTED_SECRET]";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    /// Known token formats only (outgoing payloads).
    Tokens,
    /// Known formats and secret-looking assignments (logs).
    Broad,
}

fn token_patterns() -> &'static [Regex] {
    static RE: OnceLock<Vec<Regex>> = OnceLock::new();
    RE.get_or_init(|| {
        [
            // PEM private keys, whole block.
            r"-----BEGIN [A-Z ]*PRIVATE KEY-----[\s\S]*?-----END [A-Z ]*PRIVATE KEY-----",
            // OpenAI, Anthropic and compatible `sk-` keys (also `sk-proj-`, `sk-ant-`).
            r"\bsk-[A-Za-z0-9_\-]{20,}",
            // Hugging Face and Harness tokens.
            r"\bhf_[A-Za-z0-9]{20,}",
            r"\bhs_[A-Za-z0-9_\-]{20,}",
            // GitHub.
            r"\bgh[pousr]_[A-Za-z0-9]{30,}",
            r"\bgithub_pat_[A-Za-z0-9_]{40,}",
            // AWS access key id, Google API key, Slack token.
            r"\bAKIA[0-9A-Z]{16}\b",
            r"\bAIza[0-9A-Za-z_\-]{35}\b",
            r"\bxox[abprs]-[A-Za-z0-9\-]{10,}",
            // JWT: three base64url parts, the first two start like JSON objects.
            r"\beyJ[A-Za-z0-9_\-]{8,}\.eyJ[A-Za-z0-9_\-]{8,}\.[A-Za-z0-9_\-]{8,}",
        ]
        .iter()
        .map(|p| Regex::new(p).expect("leakguard token pattern"))
        .collect()
    })
}

/// `Bearer <credential>` and `Authorization: <scheme> <credential>`: keeps the scheme.
fn bearer_pattern() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(r"(?i)\b(Bearer\s+)[A-Za-z0-9._~+/=\-]{16,}").expect("leakguard bearer")
    })
}

/// `api_key = "value"`, `"token": "value"`, `--api-key value`: keeps the name.
fn assignment_pattern() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(
            r#"(?i)((?:api[_-]?key|access[_-]?token|auth[_-]?token|secret(?:[_-]?key)?|client[_-]?secret|password|passwd)["']?\s*[:=]\s*["']?|--api-key[ =])[^\s"',;&]{6,}"#,
        )
        .expect("leakguard assignment")
    })
}

/// Replaces credentials in `text`. Returns the clean text and how many were replaced.
pub fn redact(text: &str, mode: Mode) -> (String, usize) {
    redact_with(text, mode, &[])
}

/// [`redact`], plus every exact value in `secrets` (the user's own stored keys; values shorter
/// than 8 characters are ignored so a short word cannot blank out a prompt).
pub fn redact_with(text: &str, mode: Mode, secrets: &[&str]) -> (String, usize) {
    let mut out = text.to_string();
    let mut count = 0;
    for s in secrets.iter().filter(|s| s.len() >= 8) {
        let n = out.matches(s).count();
        if n > 0 {
            out = out.replace(s, MARKER);
            count += n;
        }
    }
    for re in token_patterns() {
        let n = re.find_iter(&out).count();
        if n > 0 {
            out = re.replace_all(&out, MARKER).into_owned();
            count += n;
        }
    }
    let bearer = bearer_pattern();
    let n = bearer.find_iter(&out).count();
    if n > 0 {
        out = bearer
            .replace_all(&out, format!("${{1}}{MARKER}"))
            .into_owned();
        count += n;
    }
    if mode == Mode::Broad {
        let assign = assignment_pattern();
        // Already-redacted values do not count twice.
        let n = assign
            .captures_iter(&out)
            .filter(|c| !c.get(0).is_some_and(|m| m.as_str().ends_with(MARKER)))
            .count();
        if n > 0 {
            out = assign
                .replace_all(&out, |c: &regex::Captures| {
                    if c[0].ends_with(MARKER) {
                        c[0].to_string()
                    } else {
                        format!("{}{MARKER}", &c[1])
                    }
                })
                .into_owned();
            count += n;
        }
    }
    (out, count)
}

/// Redacts every string in a JSON value in place (object keys are left alone). Returns how
/// many credentials were replaced.
pub fn redact_value(value: &mut Value, mode: Mode) -> usize {
    match value {
        Value::String(s) => {
            let (clean, n) = redact(s, mode);
            if n > 0 {
                *s = clean;
            }
            n
        }
        Value::Array(a) => a.iter_mut().map(|v| redact_value(v, mode)).sum(),
        Value::Object(o) => o.values_mut().map(|v| redact_value(v, mode)).sum(),
        _ => 0,
    }
}

/// `true` when `text` holds something [`redact`] would replace.
pub fn contains_secret(text: &str, mode: Mode) -> bool {
    redact(text, mode).1 > 0
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn known_token_formats_are_replaced() {
        let samples = [
            "sk-proj-abcdefghijklmnopqrstuvwxyz0123456789",
            "sk-ant-api03-abcdefghijklmnopqrstuvwxyz",
            "hf_abcdefghijklmnopqrstuvwxyzABCDEF",
            "ghp_abcdefghijklmnopqrstuvwxyz0123456789",
            "AKIAABCDEFGHIJKLMNOP",
            "xoxb-1234567890-abcdefghij",
            "eyJhbGciOiJIUzI1NiJ9.eyJzdWIiOiIxMjM0NTY3ODkwIn0.dBjftJeZ4CVPmB92K27uhbUJU1p1r",
        ];
        for s in samples {
            let (out, n) = redact(&format!("the key is {s} ok"), Mode::Tokens);
            assert_eq!(out, format!("the key is {MARKER} ok"), "{s}");
            assert_eq!(n, 1);
        }
    }

    #[test]
    fn pem_private_key_block_is_one_secret() {
        let pem =
            "a\n-----BEGIN RSA PRIVATE KEY-----\nMIIBOg\nabc\n-----END RSA PRIVATE KEY-----\nb";
        let (out, n) = redact(pem, Mode::Tokens);
        assert_eq!((out.as_str(), n), ("a\n[REDACTED_SECRET]\nb", 1));
    }

    #[test]
    fn bearer_keeps_the_scheme_and_drops_the_credential() {
        let (out, n) = redact(
            "Authorization: Bearer abcdefghijklmnop1234567890",
            Mode::Tokens,
        );
        assert_eq!(out, format!("Authorization: Bearer {MARKER}"));
        assert_eq!(n, 1);
    }

    #[test]
    fn ordinary_text_and_code_are_untouched() {
        let text = "Set password = hunter2 in the form; sk-short; the hf_ prefix; Bearer token";
        assert_eq!(redact(text, Mode::Tokens), (text.to_string(), 0));
        assert!(!contains_secret("let x = 42; // eyJ", Mode::Broad));
    }

    #[test]
    fn broad_mode_catches_assignments_but_tokens_mode_does_not() {
        let text = r#"{"api_key": "abcdef123456", "name": "x"} --api-key s3cr3tvalue"#;
        assert_eq!(redact(text, Mode::Tokens).1, 0);
        let (out, n) = redact(text, Mode::Broad);
        assert_eq!(n, 2);
        assert!(out.contains(&format!("\"api_key\": \"{MARKER}")), "{out}");
        assert!(out.contains(&format!("--api-key {MARKER}")), "{out}");
        assert!(!out.contains("abcdef123456") && !out.contains("s3cr3tvalue"));
    }

    #[test]
    fn broad_mode_does_not_count_a_token_twice() {
        let (out, n) = redact("token=sk-abcdefghijklmnopqrstuvwxyz012345", Mode::Broad);
        assert_eq!(n, 1, "{out}");
        assert!(!out.contains("abcdefghijklmnop"));
    }

    #[test]
    fn exact_secrets_are_removed_but_short_ones_are_ignored() {
        let (out, n) = redact_with(
            "use zyxwvutsrqpo and abc",
            Mode::Tokens,
            &["zyxwvutsrqpo", "abc"],
        );
        assert_eq!(out, format!("use {MARKER} and abc"));
        assert_eq!(n, 1);
    }

    #[test]
    fn json_strings_are_redacted_in_place_and_keys_are_kept() {
        let mut v = json!({
            "model": "m",
            "messages": [{"role": "user", "content": "key sk-abcdefghijklmnopqrstuvwxyz012345 please"}],
            "n": 3
        });
        assert_eq!(redact_value(&mut v, Mode::Tokens), 1);
        assert_eq!(v["messages"][0]["content"], format!("key {MARKER} please"));
        assert_eq!(v["n"], 3);
    }
}
