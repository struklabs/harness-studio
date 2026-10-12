//! llama-server parameter catalog (`params.toml`, embedded) and the generic renderer that
//! turns catalog values into command-line arguments for a specific binary.
//!
//! The value types ([`ParamValue`], [`ParamValues`]) are deliberately generic so the same
//! map can later hold a ModelDefinition's `load` settings.

use crate::capabilities::RuntimeCapabilities;
use crate::error_key::{err, ErrorKey};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::sync::OnceLock;

/// A catalog value. Untagged so presets stay plain JSON (`{"jinja": true, "cache_ram": 4096}`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub enum ParamValue {
    Bool(bool),
    Int(i64),
    Float(f64),
    Text(String),
    List(Vec<String>),
}

/// Catalog id -> value.
pub type ParamValues = BTreeMap<String, ParamValue>;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ParamKind {
    Bool,
    Int,
    Float,
    Enum,
    MultiEnum,
    String,
    Path,
    List,
    Json,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ParamLevel {
    Basic,
    Advanced,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ValueFallback {
    pub value: String,
    pub args: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ParamSpec {
    pub id: String,
    pub flags: Vec<String>,
    #[serde(default)]
    pub off_flags: Vec<String>,
    pub kind: ParamKind,
    #[serde(default)]
    pub values: Vec<String>,
    #[serde(default)]
    pub values_from_help: bool,
    pub group: String,
    pub level: ParamLevel,
    pub label_key: String,
    pub help_key: String,
    #[serde(default)]
    pub requires: Vec<String>,
    #[serde(default)]
    pub conflicts: Vec<String>,
    #[serde(default)]
    pub deprecated_by: Option<String>,
    #[serde(default)]
    pub legacy_field: Option<String>,
    #[serde(default)]
    pub feature: Option<String>,
    #[serde(default)]
    pub min: Option<f64>,
    #[serde(default)]
    pub max: Option<f64>,
    #[serde(default)]
    pub separator: Option<String>,
    #[serde(default)]
    pub loopback_only: bool,
    #[serde(default)]
    pub value_fallbacks: Vec<ValueFallback>,
    #[serde(default)]
    pub placeholder: Option<String>,
    /// Value Harness sends when the user left the param unset (memory-bound llama-server
    /// defaults that Harness lowers; see [`with_harness_defaults`]).
    #[serde(default)]
    pub harness_default: Option<i64>,
}

impl ParamSpec {
    /// Rendered by the typed `LaunchConfig` code, not by [`render_params`].
    pub fn is_legacy(&self) -> bool {
        self.legacy_field.is_some()
    }

    /// Allowed values for this binary (help values + the catalog's extra ones).
    pub fn allowed_values(&self, caps: Option<&RuntimeCapabilities>) -> Vec<String> {
        let mut out: Vec<String> = Vec::new();
        if self.values_from_help {
            if let Some(info) = caps.and_then(|c| {
                self.flags
                    .iter()
                    .find_map(|f| c.flag(f).filter(|i| !i.removed))
            }) {
                out.extend(info.enum_values.iter().cloned());
            }
        }
        for v in &self.values {
            if !out.contains(v) {
                out.push(v.clone());
            }
        }
        out
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ParamCatalog {
    pub schema_version: u32,
    #[serde(rename = "param")]
    pub params: Vec<ParamSpec>,
}

impl ParamCatalog {
    pub fn get(&self, id: &str) -> Option<&ParamSpec> {
        self.params.iter().find(|p| p.id == id)
    }

    /// Catalog id that owns a flag spelling, if any.
    pub fn id_for_flag(&self, spelling: &str) -> Option<&str> {
        self.params
            .iter()
            .find(|p| {
                p.flags.iter().any(|f| f == spelling) || p.off_flags.iter().any(|f| f == spelling)
            })
            .map(|p| p.id.as_str())
    }
}

/// The embedded catalog, parsed once.
pub fn catalog() -> &'static ParamCatalog {
    static CATALOG: OnceLock<ParamCatalog> = OnceLock::new();
    CATALOG.get_or_init(|| {
        toml::from_str(include_str!("params.toml")).expect("params.toml is valid (unit tested)")
    })
}

/// Flags only Harness Studio's own fields may set (security-relevant or managed elsewhere).
/// Custom args and catalog params can never emit them.
pub const PROTECTED_FLAGS: &[&str] = &[
    "--host",
    "--port",
    "--api-key",
    "--api-key-file",
    "--mcp-servers-config",
    "--mcp-servers-json",
    "--tools",
    "--agent",
    "-ag",
    "--cors-origins",
];

/// `--x=y` / `--x_y` spellings map to the same protected flag.
pub fn is_protected(arg: &str) -> bool {
    let flag = arg.split_once('=').map_or(arg, |(flag, _)| flag);
    PROTECTED_FLAGS.contains(&flag.replace('_', "-").as_str())
}

/// Why a set param was not emitted.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SkippedParam {
    pub id: String,
    /// `unknown_param`, `legacy_field`, `unsupported_by_runtime`, `runtime_unknown`,
    /// `loopback_only`, `requires:<id>`
    pub reason: String,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Rendered {
    pub args: Vec<String>,
    pub skipped: Vec<SkippedParam>,
}

/// Launch context the renderer needs besides the values.
#[derive(Debug, Clone, Default)]
pub struct RenderContext {
    /// Bind host (for `loopback_only`).
    pub host: String,
    pub router_mode: bool,
    /// Typed `LaunchConfig` fields that are set (for `legacy:<field>` requires/conflicts).
    pub legacy_set: BTreeSet<String>,
}

fn is_loopback(host: &str) -> bool {
    crate::network::is_loopback_host(host)
}

/// A value is "set" (empty strings/lists mean "use the server default").
fn is_set(v: &ParamValue) -> bool {
    match v {
        ParamValue::Text(s) => !s.trim().is_empty(),
        ParamValue::List(l) => l.iter().any(|s| !s.trim().is_empty()),
        _ => true,
    }
}

fn number(v: &ParamValue) -> Option<f64> {
    match v {
        ParamValue::Int(i) => Some(*i as f64),
        ParamValue::Float(f) => Some(*f),
        ParamValue::Text(s) => s.trim().parse().ok(),
        _ => None,
    }
}

/// `params.invalid_value` of `spec`; the reason is a nested key (`params.expected_int`...).
fn invalid(spec: &ParamSpec, reason: ErrorKey) -> String {
    err("params.invalid_value")
        .with("id", &spec.id)
        .with("error", reason)
        .into()
}

/// Converts a value to the argument text(s) after the flag; `None` = emit nothing.
fn value_args(
    spec: &ParamSpec,
    value: &ParamValue,
    caps: &RuntimeCapabilities,
) -> Result<Option<Vec<String>>, String> {
    let check_range = |n: f64| -> Result<(), String> {
        if spec.min.is_some_and(|m| n < m) || spec.max.is_some_and(|m| n > m) {
            return Err(invalid(spec, err("params.out_of_range").with("value", n)));
        }
        Ok(())
    };
    let text = |s: &str| -> Result<String, String> {
        let s = s.trim();
        if s.contains(['\n', '\r', '\0']) {
            return Err(invalid(spec, err("params.no_line_breaks")));
        }
        Ok(s.to_string())
    };
    match spec.kind {
        ParamKind::Bool => match value {
            ParamValue::Bool(_) => Ok(Some(Vec::new())),
            _ => Err(invalid(spec, err("params.expected_bool"))),
        },
        ParamKind::Int => {
            let n = number(value).ok_or_else(|| invalid(spec, err("params.expected_int")))?;
            if n.fract() != 0.0 {
                return Err(invalid(spec, err("params.expected_int")));
            }
            check_range(n)?;
            Ok(Some(vec![(n as i64).to_string()]))
        }
        ParamKind::Float => {
            let n = number(value).ok_or_else(|| invalid(spec, err("params.expected_number")))?;
            if !n.is_finite() {
                return Err(invalid(spec, err("params.expected_number")));
            }
            check_range(n)?;
            Ok(Some(vec![n.to_string()]))
        }
        ParamKind::Enum | ParamKind::MultiEnum => {
            let items: Vec<String> = match value {
                ParamValue::Text(s) => s.split(',').map(|x| x.trim().to_string()).collect(),
                ParamValue::List(l) => l.iter().map(|x| x.trim().to_string()).collect(),
                _ => return Err(invalid(spec, err("params.expected_choice"))),
            };
            let items: Vec<String> = items.into_iter().filter(|s| !s.is_empty()).collect();
            if items.is_empty() {
                return Ok(None);
            }
            if spec.kind == ParamKind::Enum && items.len() > 1 {
                return Err(invalid(spec, err("params.expected_single_choice")));
            }
            let allowed = spec.allowed_values(Some(caps));
            for item in &items {
                let known = allowed.iter().any(|a| a == item);
                // Values the help doesn't enumerate are accepted if they are plain tokens.
                let token = item
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '+' | '.'));
                if !(known || (allowed.is_empty() && token)) {
                    return Err(invalid(
                        spec,
                        err("params.unsupported_choice").with("value", item),
                    ));
                }
            }
            Ok(Some(vec![items.join(",")]))
        }
        ParamKind::String | ParamKind::Path => match value {
            ParamValue::Text(s) => Ok(Some(vec![text(s)?])),
            _ => Err(invalid(spec, err("params.expected_text"))),
        },
        ParamKind::Json => match value {
            ParamValue::Text(s) => {
                let parsed: serde_json::Value = serde_json::from_str(s.trim())
                    .map_err(|e| invalid(spec, err("json.invalid").with("error", e)))?;
                if !parsed.is_object() {
                    return Err(invalid(spec, err("params.expected_json_object")));
                }
                Ok(Some(vec![parsed.to_string()]))
            }
            _ => Err(invalid(spec, err("params.expected_json_text"))),
        },
        ParamKind::List => {
            let items: Vec<String> = match value {
                ParamValue::List(l) => l.clone(),
                ParamValue::Text(s) => s.lines().map(str::to_string).collect(),
                _ => return Err(invalid(spec, err("params.expected_list"))),
            };
            let items = items
                .iter()
                .map(|s| text(s))
                .collect::<Result<Vec<_>, _>>()?
                .into_iter()
                .filter(|s| !s.is_empty())
                .collect::<Vec<_>>();
            if items.is_empty() {
                return Ok(None);
            }
            let sep = spec.separator.as_deref().unwrap_or(",");
            Ok(Some(vec![items.join(sep)]))
        }
    }
}

/// Adds each catalog `harness_default` the user did not choose: the param is unset in
/// `values`, none of its spellings is in `custom_args`, and the runtime has one of its flags
/// (an unknown runtime gets nothing, so older builds never see a flag they lack).
pub fn with_harness_defaults(
    catalog: &ParamCatalog,
    caps: Option<&RuntimeCapabilities>,
    values: &mut ParamValues,
    custom_args: &[String],
) {
    let Some(caps) = caps.filter(|c| c.help_parsed) else {
        return;
    };
    for spec in &catalog.params {
        let Some(default) = spec.harness_default else {
            continue;
        };
        if values.get(&spec.id).is_some_and(is_set) {
            continue;
        }
        let in_custom = custom_args.iter().any(|a| {
            let flag = a.split('=').next().unwrap_or(a);
            spec.flags.iter().chain(&spec.off_flags).any(|f| f == flag)
        });
        if in_custom || !spec.flags.iter().any(|f| caps.has_flag(f)) {
            continue;
        }
        values.insert(spec.id.clone(), ParamValue::Int(default));
    }
}

fn requirement_met(req: &str, values: &ParamValues, ctx: &RenderContext) -> bool {
    if req == "mode:router" {
        return ctx.router_mode;
    }
    if let Some(field) = req.strip_prefix("legacy:") {
        return ctx.legacy_set.contains(field);
    }
    values.get(req).is_some_and(is_set)
}

/// Renders the set catalog values for `caps`. Params the binary doesn't have (or that
/// aren't in the catalog) are skipped and reported, never emitted. Invalid values,
/// conflicts and protected flags are errors.
pub fn render_params(
    catalog: &ParamCatalog,
    caps: Option<&RuntimeCapabilities>,
    values: &ParamValues,
    ctx: &RenderContext,
) -> Result<Rendered, String> {
    let mut out = Rendered::default();
    let skip = |out: &mut Rendered, id: &str, reason: String| {
        out.skipped.push(SkippedParam {
            id: id.to_string(),
            reason,
        })
    };
    // Catalog order keeps the command line stable regardless of map order.
    for (id, _) in values.iter().filter(|(_, v)| is_set(v)) {
        if catalog.get(id).is_none() {
            skip(&mut out, id, "unknown_param".into());
        }
    }
    for spec in &catalog.params {
        let Some(value) = values.get(&spec.id).filter(|v| is_set(v)) else {
            continue;
        };
        if spec.is_legacy() {
            skip(&mut out, &spec.id, "legacy_field".into());
            continue;
        }
        for c in &spec.conflicts {
            if requirement_met(c, values, ctx) {
                let other = c.strip_prefix("legacy:").unwrap_or(c);
                return Err(err("params.conflict")
                    .with("id", &spec.id)
                    .with("other", other)
                    .into());
            }
        }
        if let Some(missing) = spec
            .requires
            .iter()
            .find(|r| !requirement_met(r, values, ctx))
        {
            skip(&mut out, &spec.id, format!("requires:{}", missing));
            continue;
        }
        if spec.loopback_only && !is_loopback(&ctx.host) {
            skip(&mut out, &spec.id, "loopback_only".into());
            continue;
        }
        let Some(caps) = caps.filter(|c| c.help_parsed) else {
            skip(&mut out, &spec.id, "runtime_unknown".into());
            continue;
        };
        let chosen = if let ParamValue::Bool(false) = value {
            spec.off_flags.iter().find(|f| caps.has_flag(f))
        } else {
            spec.flags.iter().find(|f| caps.has_flag(f))
        };
        let args: Vec<String> = match chosen {
            Some(flag) => match value_args(spec, value, caps)? {
                Some(rest) => std::iter::once(flag.clone()).chain(rest).collect(),
                None => continue,
            },
            None if matches!(value, ParamValue::Bool(false))
                && spec.off_flags.is_empty()
                && spec.flags.iter().any(|f| caps.has_flag(f)) =>
            {
                // A switch without a negative form: "off" is the default, nothing to emit.
                continue;
            }
            None => {
                let text = match value {
                    ParamValue::Text(s) => Some(s.trim()),
                    _ => None,
                };
                let fallback = text.and_then(|t| {
                    spec.value_fallbacks
                        .iter()
                        .find(|f| f.value == t)
                        .filter(|f| {
                            f.args
                                .iter()
                                .filter(|a| a.starts_with('-'))
                                .all(|a| caps.has_flag(a))
                        })
                });
                match fallback {
                    Some(f) => f.args.clone(),
                    None => {
                        skip(&mut out, &spec.id, "unsupported_by_runtime".into());
                        continue;
                    }
                }
            }
        };
        if let Some(bad) = args.iter().find(|a| a.starts_with('-') && is_protected(a)) {
            return Err(err("params.protected_flag").with("flag", bad).into());
        }
        out.args.extend(args);
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::capabilities::testdata;

    fn vals(pairs: &[(&str, ParamValue)]) -> ParamValues {
        pairs
            .iter()
            .map(|(k, v)| (k.to_string(), v.clone()))
            .collect()
    }

    fn ctx() -> RenderContext {
        RenderContext {
            host: "127.0.0.1".into(),
            ..Default::default()
        }
    }

    #[test]
    fn catalog_is_valid() {
        let cat = catalog();
        assert_eq!(cat.schema_version, 1);
        let mut ids = BTreeSet::new();
        for p in &cat.params {
            assert!(ids.insert(p.id.clone()), "duplicate id {}", p.id);
            assert!(!p.flags.is_empty(), "{} has no flags", p.id);
            for f in p.flags.iter().chain(&p.off_flags) {
                assert!(f.starts_with('-'), "{}: {}", p.id, f);
            }
            if !p.is_legacy() {
                for f in p.flags.iter().chain(&p.off_flags) {
                    assert!(!is_protected(f), "{}: catalog must not emit {}", p.id, f);
                }
                for fb in &p.value_fallbacks {
                    assert!(fb.args.iter().all(|a| !is_protected(a)), "{}", p.id);
                }
            }
            if matches!(p.kind, ParamKind::Enum | ParamKind::MultiEnum) {
                assert!(
                    p.values_from_help || !p.values.is_empty(),
                    "{}: enum without values",
                    p.id
                );
            }
            for r in p.requires.iter().chain(&p.conflicts) {
                let known = r == "mode:router" || r.starts_with("legacy:") || cat.get(r).is_some();
                assert!(known, "{}: unknown reference {}", p.id, r);
            }
            if let Some(d) = &p.deprecated_by {
                assert!(cat.get(d).is_some(), "{}: deprecated_by {}", p.id, d);
            }
            assert!(p.label_key.contains('.') && p.help_key.contains('.'));
        }
    }

    /// Every catalog entry exists in the current real build (catches typos), and every
    /// candidate spelling that a real build has parses to a usable flag with the kind's shape.
    #[test]
    fn catalog_flags_exist_in_real_help() {
        let new = testdata::caps("b11236");
        let old = testdata::caps("b11026");
        for p in &catalog().params {
            assert!(
                p.flags.iter().any(|f| new.has_flag(f)),
                "{}: none of {:?} in b11236",
                p.id,
                p.flags
            );
            for caps in [&new, &old] {
                for f in p.flags.iter().chain(&p.off_flags) {
                    let Some(info) = caps.flag(f) else { continue };
                    if info.removed {
                        continue;
                    }
                    let takes_value = info.value_hint.is_some();
                    match p.kind {
                        ParamKind::Bool if p.legacy_field.is_none() => {
                            assert!(!takes_value, "{}: {} takes a value", p.id, f)
                        }
                        ParamKind::Int
                        | ParamKind::Float
                        | ParamKind::String
                        | ParamKind::Path
                        | ParamKind::List
                        | ParamKind::Json
                        | ParamKind::MultiEnum => {
                            assert!(takes_value, "{}: {} takes no value", p.id, f)
                        }
                        _ => {}
                    }
                }
            }
            if p.values_from_help {
                assert!(
                    !p.allowed_values(Some(&new)).is_empty(),
                    "{}: no values parsed from help",
                    p.id
                );
            }
        }
    }

    /// help_key/label_key of every entry and every feature reason exist in the locale files
    /// (tr and en both; `npm run check:locales` enforces that the two sets are equal).
    #[test]
    fn i18n_keys_exist_in_locales() {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../src/lib/locales");
        let mut text = String::new();
        for entry in std::fs::read_dir(&dir).unwrap() {
            let path = entry.unwrap().path();
            if path.extension().is_some_and(|e| e == "ts") {
                text.push_str(&std::fs::read_to_string(path).unwrap());
            }
        }
        let count = |key: &str| text.matches(&format!("'{}':", key)).count();
        let mut keys: Vec<String> = Vec::new();
        for p in &catalog().params {
            keys.push(p.label_key.clone());
            keys.push(p.help_key.clone());
            keys.push(format!("caps.group.{}", p.group));
        }
        for r in crate::capabilities::features::REASON_KEYS {
            keys.push(format!("caps.reason.{}", r));
        }
        for k in keys {
            assert!(count(&k) >= 2, "locale key {} missing (tr+en)", k);
        }
    }

    #[test]
    fn renders_first_supported_spelling_in_catalog_order() {
        let caps = testdata::caps("b11236");
        let v = vals(&[
            ("spec_draft_n_max", ParamValue::Int(8)),
            ("jinja", ParamValue::Bool(false)),
            ("kv_offload", ParamValue::Bool(false)),
            ("cache_ram", ParamValue::Int(4096)),
            ("reasoning_format", ParamValue::Text("deepseek".into())),
            (
                "spec_type",
                ParamValue::List(vec!["ngram-mod".into(), "draft-simple".into()]),
            ),
            (
                "chat_template_kwargs",
                ParamValue::Text(r#"{ "enable_thinking": false }"#.into()),
            ),
            (
                "override_tensor",
                ParamValue::List(vec!["a=CPU".into(), "".into(), "b=CPU".into()]),
            ),
            ("fit", ParamValue::Text("off".into())),
            ("swa_full", ParamValue::Bool(false)),
            ("cors_origins", ParamValue::Text("   ".into())),
        ]);
        let r = render_params(catalog(), Some(&caps), &v, &ctx()).unwrap();
        assert!(r.skipped.is_empty(), "{:?}", r.skipped);
        assert_eq!(
            r.args,
            vec![
                "--fit",
                "off",
                "--no-kv-offload",
                "--cache-ram",
                "4096",
                "--override-tensor",
                "a=CPU,b=CPU",
                "--no-jinja",
                "--chat-template-kwargs",
                r#"{"enable_thinking":false}"#,
                "--reasoning-format",
                "deepseek",
                "--spec-type",
                "ngram-mod,draft-simple",
                "--spec-draft-n-max",
                "8",
            ]
        );
    }

    #[test]
    fn rope_yarn_and_template_file_flags_are_rendered() {
        let caps = testdata::caps("b11236");
        let v = vals(&[
            ("yarn_beta_fast", ParamValue::Float(32.0)),
            ("yarn_beta_slow", ParamValue::Float(1.0)),
            ("yarn_attn_factor", ParamValue::Float(0.8)),
            ("yarn_ext_factor", ParamValue::Float(1.0)),
            ("yarn_orig_ctx", ParamValue::Int(32768)),
            ("rope_scale", ParamValue::Float(4.0)),
            (
                "chat_template_file",
                ParamValue::Text("C:/t/custom.jinja".into()),
            ),
        ]);
        let r = render_params(catalog(), Some(&caps), &v, &ctx()).unwrap();
        assert!(r.skipped.is_empty(), "{:?}", r.skipped);
        assert_eq!(
            r.args,
            vec![
                "--rope-scale",
                "4",
                "--chat-template-file",
                "C:/t/custom.jinja",
                "--yarn-orig-ctx",
                "32768",
                "--yarn-ext-factor",
                "1",
                "--yarn-attn-factor",
                "0.8",
                "--yarn-beta-slow",
                "1",
                "--yarn-beta-fast",
                "32",
            ]
        );
    }

    #[test]
    fn override_kv_list_emits_supported_flag() {
        let caps = testdata::caps("b11236");
        let values = vals(&[(
            "override_kv",
            ParamValue::List(vec!["a=int:1".into(), "b=str:x".into()]),
        )]);
        let rendered = render_params(catalog(), Some(&caps), &values, &ctx()).unwrap();
        assert!(rendered.skipped.is_empty());
        assert_eq!(rendered.args, ["--override-kv", "a=int:1,b=str:x"]);
        let mut unsupported = caps;
        unsupported.flags.remove("--override-kv");
        let rendered = render_params(catalog(), Some(&unsupported), &values, &ctx()).unwrap();
        assert!(rendered.args.is_empty());
        assert_eq!(rendered.skipped.len(), 1);
    }

    #[test]
    fn removed_and_missing_flags_are_skipped_not_emitted() {
        let mut caps = testdata::caps("b11236");
        // Pretend this build lacks --swa-full and --load-mode (older runtime).
        caps.flags
            .retain(|k, _| k != "--swa-full" && k != "--load-mode");
        let caps = crate::capabilities::RuntimeCapabilities::from_outputs(
            caps.key.clone(),
            &testdata::read("llama-server-b11236.version.txt"),
            &testdata::read("llama-server-b11236.help.txt")
                .lines()
                .filter(|l| !l.starts_with("--swa-full") && !l.starts_with("-lm,"))
                .collect::<Vec<_>>()
                .join("\n"),
            "",
        );
        assert!(!caps.has_flag("--swa-full"));
        let v = vals(&[
            ("swa_full", ParamValue::Bool(true)),
            ("load_mode", ParamValue::Text("dio".into())),
            ("made_up", ParamValue::Int(1)),
            ("flash_attn", ParamValue::Bool(true)),
        ]);
        let r = render_params(catalog(), Some(&caps), &v, &ctx()).unwrap();
        assert!(r.args.is_empty(), "{:?}", r.args);
        let reasons: BTreeMap<_, _> = r
            .skipped
            .iter()
            .map(|s| (s.id.as_str(), s.reason.as_str()))
            .collect();
        assert_eq!(reasons["swa_full"], "unsupported_by_runtime");
        assert_eq!(reasons["load_mode"], "unsupported_by_runtime");
        assert_eq!(reasons["made_up"], "unknown_param");
        assert_eq!(reasons["flash_attn"], "legacy_field");
    }

    #[test]
    fn load_mode_legacy_fallback() {
        // An old-style runtime with --no-mmap/--mlock but no --load-mode.
        let help = "-ngl, --gpu-layers N   layers\n--no-mmap   do not memory-map model\n--mlock   keep model in RAM\n";
        let caps = crate::capabilities::RuntimeCapabilities::from_outputs(
            Default::default(),
            "",
            help,
            "",
        );
        let r = render_params(
            catalog(),
            Some(&caps),
            &vals(&[("load_mode", ParamValue::Text("mlock".into()))]),
            &ctx(),
        )
        .unwrap();
        assert_eq!(r.args, vec!["--no-mmap", "--mlock"]);
        let r = render_params(
            catalog(),
            Some(&caps),
            &vals(&[("load_mode", ParamValue::Text("dio".into()))]),
            &ctx(),
        )
        .unwrap();
        assert_eq!(r.skipped[0].reason, "unsupported_by_runtime");
    }

    #[test]
    fn validation_conflicts_and_policies() {
        let caps = testdata::caps("b11236");
        let err = |v: ParamValues, c: &RenderContext| {
            render_params(catalog(), Some(&caps), &v, c).unwrap_err()
        };
        // `params.invalid_value` with the reason key nested in `error`.
        let reason = |e: String| {
            let (key, params) = crate::error_key::parse(&e).unwrap();
            assert_eq!(key, "params.invalid_value", "{e}");
            let inner = params["error"].as_str().unwrap().to_string();
            crate::error_key::parse(&inner).unwrap().0.to_string()
        };
        assert_eq!(
            reason(err(vals(&[("cache_ram", ParamValue::Int(-5))]), &ctx())),
            "params.out_of_range"
        );
        assert_eq!(
            reason(err(vals(&[("cache_ram", ParamValue::Float(1.5))]), &ctx())),
            "params.expected_int"
        );
        assert_eq!(
            reason(err(
                vals(&[("pooling", ParamValue::Text("bogus".into()))]),
                &ctx()
            )),
            "params.unsupported_choice"
        );
        assert_eq!(
            reason(err(
                vals(&[("chat_template_kwargs", ParamValue::Text("[1]".into()))]),
                &ctx()
            )),
            "params.expected_json_object"
        );
        assert_eq!(
            reason(err(
                vals(&[("fit_target", ParamValue::Text("a\nb".into()))]),
                &ctx()
            )),
            "params.no_line_breaks"
        );
        let conflict = err(
            vals(&[
                ("cpu_moe", ParamValue::Bool(true)),
                ("n_cpu_moe", ParamValue::Int(4)),
            ]),
            &ctx(),
        );
        assert_eq!(
            crate::error_key::parse(&conflict).unwrap().0,
            "params.conflict",
            "{conflict}"
        );
        let with_lora = RenderContext {
            legacy_set: BTreeSet::from(["lora_path".to_string()]),
            ..ctx()
        };
        assert!(err(
            vals(&[("lora_adapters", ParamValue::List(vec!["a.gguf".into()]))]),
            &with_lora
        )
        .contains("lora_path"));

        // --props only on loopback; router params only in router mode.
        let public = RenderContext {
            host: "0.0.0.0".into(),
            ..ctx()
        };
        let v = vals(&[
            ("props", ParamValue::Bool(true)),
            ("models_max", ParamValue::Int(2)),
        ]);
        let r = render_params(catalog(), Some(&caps), &v, &public).unwrap();
        assert!(r.args.is_empty());
        let reasons: BTreeMap<_, _> = r
            .skipped
            .iter()
            .map(|s| (s.id.as_str(), s.reason.as_str()))
            .collect();
        assert_eq!(reasons["models_max"], "requires:mode:router");
        assert_eq!(reasons["props"], "loopback_only");
        let router = RenderContext {
            router_mode: true,
            ..ctx()
        };
        let r = render_params(catalog(), Some(&caps), &v, &router).unwrap();
        assert_eq!(r.args, vec!["--props", "--models-max", "2"]);

        // Without runtime information nothing is emitted.
        let r = render_params(
            catalog(),
            None,
            &vals(&[("jinja", ParamValue::Bool(true))]),
            &ctx(),
        )
        .unwrap();
        assert!(r.args.is_empty());
        assert_eq!(r.skipped[0].reason, "runtime_unknown");
    }

    #[test]
    fn param_values_json_roundtrip() {
        let json = r#"{"jinja":true,"cache_ram":4096,"spec_draft_p_min":0.5,"fit":"on","lora_adapters":["a.gguf"]}"#;
        let v: ParamValues = serde_json::from_str(json).unwrap();
        assert_eq!(v["jinja"], ParamValue::Bool(true));
        assert_eq!(v["cache_ram"], ParamValue::Int(4096));
        assert_eq!(v["spec_draft_p_min"], ParamValue::Float(0.5));
        assert_eq!(v["lora_adapters"], ParamValue::List(vec!["a.gguf".into()]));
        let again: ParamValues = serde_json::from_str(&serde_json::to_string(&v).unwrap()).unwrap();
        assert_eq!(again, v);
    }
}
