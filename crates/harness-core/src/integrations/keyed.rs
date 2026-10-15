//! Key-level config edits with a ledger (rollback safety).
//!
//! An integration describes *what* it writes as a list of [`FilePlan`]s: for a config file, the
//! keys it sets ([`Edit`]: a path of [`Seg`]s and a JSON value); for a file Harness owns
//! entirely, the whole content. This module does the rest for every format:
//!
//! * **Comment-preserving edits.** JSON/JSONC is edited on a concrete syntax tree
//!   (`jsonc-parser`), TOML with `toml_edit`: comments, key order and formatting the user wrote
//!   stay. YAML is re-serialized (comments are lost; the result says so and a backup is kept).
//! * **A ledger** (`<data>/config/integrations.json`): per integration and file, the keys Harness
//!   set (a SHA-256 of the value written, the value that was there before, which parent objects
//!   Harness created) and a SHA-256 of the file as written. Its location scopes ownership to
//!   the data directory; automatic edits require the recorded entry to still match.
//! * **Rollback** removes or restores only those keys, and only while they still hold what
//!   Harness wrote; a key the user changed afterwards is left alone and reported. Without a
//!   ledger entry (configs written by older versions) only keys that identify Harness by name
//!   ([`Edit::marker`], e.g. `provider.harness`) are removed.
//! * **Drift warning**: when the file's hash differs from the one recorded at apply time, the
//!   list, a re-apply and the rollback report it.
//! * **Backups**: before every write `<file>.harness-<YYYYMMDD-HHMMSS>.bak`; the newest
//!   [`BACKUP_GENERATIONS`] per file are kept.
//!
//! Nothing is written when any file of a plan fails to parse (all files are rendered first).

use std::path::{Path, PathBuf};

use crate::error_key::err;
use jsonc_parser::cst::{CstArray, CstInputValue, CstNode, CstObject, CstRootNode};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use sha2::{Digest, Sha256};

/// Backups kept per config file.
pub const BACKUP_GENERATIONS: usize = 5;

/// One step of a key path.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Seg {
    /// An object key.
    Key(String),
    /// The array element (an object) whose `field` equals `value`; appended when absent.
    Item { field: String, value: String },
}

impl Seg {
    pub fn key(k: &str) -> Self {
        Seg::Key(k.to_string())
    }
    pub fn item(field: &str, value: &str) -> Self {
        Seg::Item {
            field: field.to_string(),
            value: value.to_string(),
        }
    }
}

/// `a.b.c` -> key path.
pub fn keys(dotted: &[&str]) -> Vec<Seg> {
    dotted.iter().map(|k| Seg::key(k)).collect()
}

pub fn path_text(path: &[Seg]) -> String {
    path.iter()
        .map(|s| match s {
            Seg::Key(k) => k.clone(),
            Seg::Item { field, value } => format!("[{}={}]", field, value),
        })
        .collect::<Vec<_>>()
        .join(".")
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Format {
    Json,
    Toml,
    Yaml,
}

/// A key Harness sets.
#[derive(Debug, Clone)]
pub struct Edit {
    pub path: Vec<Seg>,
    pub value: Value,
    /// An existing value is replaced only when this returns true (e.g. keep a default model
    /// the user picked). `None` = always replace.
    pub replace_if: Option<fn(&Value) -> bool>,
    /// The key names Harness itself (`provider.harness`): safe to remove without a ledger.
    pub marker: bool,
}

impl Edit {
    pub fn set(path: Vec<Seg>, value: Value) -> Self {
        Edit {
            path,
            value,
            replace_if: None,
            marker: false,
        }
    }
    pub fn marker(path: Vec<Seg>, value: Value) -> Self {
        Edit {
            marker: true,
            ..Edit::set(path, value)
        }
    }
    pub fn only_if(mut self, f: fn(&Value) -> bool) -> Self {
        self.replace_if = Some(f);
        self
    }
}

/// What an integration writes into one file.
#[derive(Debug, Clone)]
pub enum FilePlan {
    Keys {
        path: PathBuf,
        format: Format,
        edits: Vec<Edit>,
        /// Content of a file that does not exist yet (e.g. `{"$schema": ...}`); `None` = empty.
        new_doc: Option<Value>,
    },
    /// A file Harness owns entirely (e.g. a Goose custom provider, a Codex profile).
    Whole { path: PathBuf, content: String },
}

impl FilePlan {
    pub fn path(&self) -> &Path {
        match self {
            FilePlan::Keys { path, .. } | FilePlan::Whole { path, .. } => path,
        }
    }
}

// ---------------------------------------------------------------------------
// Ledger
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AppliedKey {
    pub path: Vec<Seg>,
    /// SHA-256 of the value Harness wrote (values can be secrets; they are not stored).
    pub value_sha: String,
    /// URL fields written inside this key, independent of the surrounding settings.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub urls: Vec<UrlHash>,
    /// What was there before the first apply (`None` = absent).
    #[serde(default)]
    pub prior: Option<Value>,
    /// Index of the first path step that did not exist before (those parents are Harness's).
    #[serde(default)]
    pub created_from: Option<usize>,
    #[serde(default)]
    pub marker: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct UrlHash {
    pub path: Vec<Seg>,
    pub value_sha: String,
}

/// Address fields in a value, using stable selectors for named array entries.
pub fn url_values(path: &[Seg], value: &Value) -> Vec<(Vec<Seg>, Value)> {
    let mut out = Vec::new();
    match value {
        Value::String(s) if s.starts_with("http://") || s.starts_with("https://") => {
            if path.last().is_some_and(|s| {
                matches!(s, Seg::Key(k) if
                matches!(k.replace('_', "").to_ascii_lowercase().as_str(),
                    "url" | "baseurl" | "apibase" | "apiurl" | "anthropicbaseurl"))
            }) {
                out.push((path.to_vec(), value.clone()));
            }
        }
        Value::Object(m) => {
            for (k, v) in m {
                let mut next = path.to_vec();
                next.push(Seg::key(k));
                out.extend(url_values(&next, v));
            }
        }
        Value::Array(a) => {
            for v in a {
                if let Some((field, name)) = ["id", "title", "name"]
                    .iter()
                    .find_map(|field| v.get(*field)?.as_str().map(|name| (*field, name)))
                {
                    let mut next = path.to_vec();
                    next.push(Seg::item(field, name));
                    out.extend(url_values(&next, v));
                }
            }
        }
        _ => {}
    }
    out
}

fn url_hashes(path: &[Seg], value: &Value) -> Vec<UrlHash> {
    url_values(path, value)
        .into_iter()
        .map(|(path, v)| UrlHash {
            path,
            value_sha: value_sha(&v),
        })
        .collect()
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LedgerFile {
    pub path: String,
    /// SHA-256 of the file as Harness wrote it.
    pub sha256: String,
    /// The file did not exist before the first apply.
    #[serde(default)]
    pub created: bool,
    /// Harness owns the whole file.
    #[serde(default)]
    pub whole: bool,
    #[serde(default)]
    pub keys: Vec<AppliedKey>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LedgerEntry {
    pub id: String,
    pub applied_at: u64,
    pub files: Vec<LedgerFile>,
    /// The apply request without its key (`integrations::StoredRequest`): lets Harness rewrite
    /// the config for a new gateway port on its own. Absent in entries written before.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub request: Option<Value>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Ledger {
    #[serde(default)]
    pub entries: Vec<LedgerEntry>,
}

impl Ledger {
    pub fn load(path: Option<&Path>) -> Ledger {
        path.map(|p| crate::store::JsonStore::sensitive(p).load_or_default())
            .unwrap_or_default()
    }

    pub fn save(&self, path: Option<&Path>) -> Result<(), String> {
        match path {
            Some(p) => crate::store::JsonStore::sensitive(p)
                .save(self)
                .map_err(|e| {
                    err("integrations.ledger_write_failed")
                        .with("error", e)
                        .into()
                }),
            None => Ok(()),
        }
    }

    pub fn entry(&self, id: &str) -> Option<&LedgerEntry> {
        self.entries.iter().find(|e| e.id == id)
    }

    fn file(&self, id: &str, path: &Path) -> Option<&LedgerFile> {
        let p = path.to_string_lossy();
        self.entry(id)?.files.iter().find(|f| f.path == p)
    }

    pub fn put(&mut self, entry: LedgerEntry) {
        self.entries.retain(|e| e.id != entry.id);
        self.entries.push(entry);
    }

    pub fn remove(&mut self, id: &str) {
        self.entries.retain(|e| e.id != id);
    }
}

pub fn sha256_hex(bytes: &[u8]) -> String {
    let mut h = Sha256::new();
    h.update(bytes);
    h.finalize()
        .iter()
        .map(|b| format!("{:02x}", b))
        .collect::<String>()
}

fn value_sha(v: &Value) -> String {
    sha256_hex(canonical(v).as_bytes())
}

/// Order-independent text of a value (objects with sorted keys).
fn canonical(v: &Value) -> String {
    match v {
        Value::Object(m) => {
            let mut keys: Vec<&String> = m.keys().collect();
            keys.sort();
            let inner: Vec<String> = keys
                .iter()
                .map(|k| format!("{}:{}", Value::String((*k).clone()), canonical(&m[*k])))
                .collect();
            format!("{{{}}}", inner.join(","))
        }
        Value::Array(a) => format!(
            "[{}]",
            a.iter().map(canonical).collect::<Vec<_>>().join(",")
        ),
        Value::Number(n) => n
            .as_f64()
            .filter(|f| f.fract() == 0.0 && f.abs() < 1e15)
            .map(|f| format!("{}", f as i64))
            .unwrap_or_else(|| n.to_string()),
        other => other.to_string(),
    }
}

// ---------------------------------------------------------------------------
// Documents
// ---------------------------------------------------------------------------

fn get_in<'a>(root: &'a Value, path: &[Seg]) -> Option<&'a Value> {
    let mut cur = root;
    for seg in path {
        cur = match seg {
            Seg::Key(k) => cur.as_object()?.get(k)?,
            Seg::Item { field, value } => cur
                .as_array()?
                .iter()
                .find(|e| e.get(field).and_then(Value::as_str) == Some(value.as_str()))?,
        };
    }
    Some(cur)
}

/// Index of the first step of `path` that does not exist in `root`.
fn first_missing(root: &Value, path: &[Seg]) -> Option<usize> {
    (0..path.len()).find(|i| get_in(root, &path[..=*i]).is_none())
}

fn is_empty_container(v: &Value) -> bool {
    match v {
        Value::Object(m) => m.is_empty(),
        Value::Array(a) => a.is_empty(),
        _ => false,
    }
}

fn shape_error(path: &[Seg], file: &Path) -> String {
    err("integrations.config_shape")
        .with("field", path_text(path))
        .with("path", file.display())
        .into()
}

enum Doc {
    Json(CstRootNode),
    Toml(toml_edit::DocumentMut),
    Yaml(Value),
}

impl Doc {
    fn parse(format: Format, text: &str, file: &Path) -> Result<Doc, String> {
        let text = text.trim_start_matches('\u{feff}');
        let bad = || super::unparseable_msg(file);
        match format {
            Format::Json => {
                let root = CstRootNode::parse(text, &jsonc_parser::ParseOptions::default())
                    .map_err(|_| bad())?;
                match root.value() {
                    None => {}
                    Some(v) if v.as_object().is_some() || v.as_array().is_some() => {}
                    Some(_) => return Err(bad()),
                }
                Ok(Doc::Json(root))
            }
            Format::Toml => text
                .parse::<toml_edit::DocumentMut>()
                .map(Doc::Toml)
                .map_err(|_| bad()),
            Format::Yaml => {
                let v = super::parse_yaml(text).map_err(|_| bad())?;
                if !(v.is_object() || v.is_array()) {
                    return Err(bad());
                }
                Ok(Doc::Yaml(v))
            }
        }
    }

    fn new(format: Format, init: Option<&Value>, path: &[Seg]) -> Doc {
        let root_array = matches!(path.first(), Some(Seg::Item { .. }));
        let init = init.cloned().unwrap_or_else(|| {
            if root_array {
                Value::Array(Vec::new())
            } else {
                Value::Object(Map::new())
            }
        });
        match format {
            Format::Json => {
                let root = CstRootNode::parse("", &jsonc_parser::ParseOptions::default())
                    .expect("empty document parses");
                root.set_value(to_cst(&init));
                Doc::Json(root)
            }
            Format::Toml => {
                let mut doc = toml_edit::DocumentMut::new();
                if let Value::Object(m) = &init {
                    for (k, v) in m {
                        doc[k.as_str()] = to_toml_item(v, true);
                    }
                }
                Doc::Toml(doc)
            }
            Format::Yaml => Doc::Yaml(init),
        }
    }

    fn value(&self) -> Value {
        match self {
            Doc::Json(root) => root
                .value()
                .and_then(|v| v.to_serde_value())
                .unwrap_or_else(|| Value::Object(Map::new())),
            Doc::Toml(doc) => table_to_json(doc.as_table()),
            Doc::Yaml(v) => v.clone(),
        }
    }

    fn text(&self) -> Result<String, String> {
        match self {
            Doc::Json(root) => {
                let mut s = root.to_string();
                if !s.ends_with('\n') {
                    s.push('\n');
                }
                Ok(s)
            }
            Doc::Toml(doc) => Ok(doc.to_string()),
            Doc::Yaml(v) => serde_norway::to_string(v).map_err(|e| e.to_string()),
        }
    }

    fn set(&mut self, path: &[Seg], value: &Value, file: &Path) -> Result<(), String> {
        match self {
            Doc::Json(root) => json_set(root, path, value).ok_or_else(|| shape_error(path, file)),
            Doc::Toml(doc) => {
                toml_set(doc.as_table_mut(), path, value).ok_or_else(|| shape_error(path, file))
            }
            Doc::Yaml(v) => value_set(v, path, value).ok_or_else(|| shape_error(path, file)),
        }
    }

    fn remove(&mut self, path: &[Seg]) -> bool {
        match self {
            Doc::Json(root) => json_remove(root, path),
            Doc::Toml(doc) => toml_remove(doc.as_table_mut(), path),
            Doc::Yaml(v) => value_remove(v, path),
        }
    }
}

// --- JSON (CST) ---

fn to_cst(v: &Value) -> CstInputValue {
    match v {
        Value::Null => CstInputValue::Null,
        Value::Bool(b) => CstInputValue::Bool(*b),
        Value::Number(n) => CstInputValue::Number(n.to_string()),
        Value::String(s) => CstInputValue::String(s.clone()),
        Value::Array(a) => CstInputValue::Array(a.iter().map(to_cst).collect()),
        Value::Object(m) => {
            CstInputValue::Object(m.iter().map(|(k, v)| (k.clone(), to_cst(v))).collect())
        }
    }
}

enum Cont {
    Obj(CstObject),
    Arr(CstArray),
}

fn find_element(arr: &CstArray, field: &str, value: &str) -> Option<CstNode> {
    arr.elements().into_iter().find(|e| {
        e.to_serde_value()
            .and_then(|v| v.get(field).and_then(Value::as_str).map(|s| s == value))
            .unwrap_or(false)
    })
}

/// The container a step leads into, creating it (as `next` needs) when missing. `None` when an
/// existing value has another shape (never overwritten).
fn json_step(cont: &Cont, seg: &Seg, next: &Seg) -> Option<Cont> {
    let want_array = matches!(next, Seg::Item { .. });
    let from_node = |node: CstNode| -> Option<Cont> {
        if want_array {
            node.as_array().map(Cont::Arr)
        } else {
            node.as_object().map(Cont::Obj)
        }
    };
    let empty = || {
        if want_array {
            CstInputValue::Array(Vec::new())
        } else {
            CstInputValue::Object(Vec::new())
        }
    };
    match (cont, seg) {
        (Cont::Obj(o), Seg::Key(k)) => match o.get(k) {
            Some(prop) => match prop.value() {
                Some(v) if v.to_serde_value().is_some_and(|x| x.is_null()) => {
                    prop.set_value(empty());
                    from_node(prop.value()?)
                }
                Some(v) => from_node(v),
                None => None,
            },
            None => {
                let prop = o.append(k, empty());
                from_node(prop.value()?)
            }
        },
        (Cont::Arr(a), Seg::Item { field, value }) => match find_element(a, field, value) {
            Some(e) => from_node(e),
            // Arrays of arrays are not used.
            None if want_array => None,
            None => from_node(a.append(CstInputValue::Object(vec![(
                field.clone(),
                CstInputValue::String(value.clone()),
            )]))),
        },
        _ => None,
    }
}

fn json_set(root: &CstRootNode, path: &[Seg], value: &Value) -> Option<()> {
    let (last, parents) = path.split_last()?;
    let root_array = matches!(path.first(), Some(Seg::Item { .. }));
    if root.value().is_none() {
        root.set_value(if root_array {
            CstInputValue::Array(Vec::new())
        } else {
            CstInputValue::Object(Vec::new())
        });
    }
    let top = root.value()?;
    let mut cont = if root_array {
        Cont::Arr(top.as_array()?)
    } else {
        Cont::Obj(top.as_object()?)
    };
    for (i, seg) in parents.iter().enumerate() {
        let next = parents.get(i + 1).unwrap_or(last);
        cont = json_step(&cont, seg, next)?;
    }
    match (&cont, last) {
        (Cont::Obj(o), Seg::Key(k)) => {
            match o.get(k) {
                Some(prop) => prop.set_value(to_cst(value)),
                None => {
                    o.append(k, to_cst(value));
                }
            }
            Some(())
        }
        (Cont::Arr(a), Seg::Item { field, value: v }) => {
            let mut obj = value.clone();
            if let Some(m) = obj.as_object_mut() {
                m.insert(field.clone(), Value::String(v.clone()));
            }
            match find_element(a, field, v) {
                Some(e) => {
                    let idx = a
                        .elements()
                        .iter()
                        .position(|x| x.to_string() == e.to_string())
                        .unwrap_or(0);
                    e.remove();
                    a.insert(idx, to_cst(&obj));
                }
                None => {
                    a.append(to_cst(&obj));
                }
            }
            Some(())
        }
        _ => None,
    }
}

fn json_remove(root: &CstRootNode, path: &[Seg]) -> bool {
    let Some((last, parents)) = path.split_last() else {
        return false;
    };
    let Some(top) = root.value() else {
        return false;
    };
    let mut cont = match (top.as_object(), top.as_array()) {
        (Some(o), _) => Cont::Obj(o),
        (_, Some(a)) => Cont::Arr(a),
        _ => return false,
    };
    for seg in parents {
        let node = match (&cont, seg) {
            (Cont::Obj(o), Seg::Key(k)) => o.get(k).and_then(|p| p.value()),
            (Cont::Arr(a), Seg::Item { field, value }) => find_element(a, field, value),
            _ => None,
        };
        let Some(node) = node else { return false };
        cont = match (node.as_object(), node.as_array()) {
            (Some(o), _) => Cont::Obj(o),
            (_, Some(a)) => Cont::Arr(a),
            _ => return false,
        };
    }
    match (&cont, last) {
        (Cont::Obj(o), Seg::Key(k)) => match o.get(k) {
            Some(p) => {
                p.remove();
                true
            }
            None => false,
        },
        (Cont::Arr(a), Seg::Item { field, value }) => match find_element(a, field, value) {
            Some(e) => {
                e.remove();
                true
            }
            None => false,
        },
        _ => false,
    }
}

// --- TOML ---

fn toml_value_to_json(v: &toml_edit::Value) -> Value {
    use toml_edit::Value as T;
    match v {
        T::String(s) => Value::String(s.value().clone()),
        T::Integer(i) => Value::from(*i.value()),
        T::Float(f) => serde_json::Number::from_f64(*f.value())
            .map(Value::Number)
            .unwrap_or(Value::Null),
        T::Boolean(b) => Value::Bool(*b.value()),
        T::Datetime(d) => Value::String(d.value().to_string()),
        T::Array(a) => Value::Array(a.iter().map(toml_value_to_json).collect()),
        T::InlineTable(t) => Value::Object(
            t.iter()
                .map(|(k, v)| (k.to_string(), toml_value_to_json(v)))
                .collect(),
        ),
    }
}

fn item_to_json(item: &toml_edit::Item) -> Option<Value> {
    match item {
        toml_edit::Item::None => None,
        toml_edit::Item::Value(v) => Some(toml_value_to_json(v)),
        toml_edit::Item::Table(t) => Some(table_to_json(t)),
        toml_edit::Item::ArrayOfTables(a) => {
            Some(Value::Array(a.iter().map(table_to_json).collect()))
        }
    }
}

fn table_to_json(t: &toml_edit::Table) -> Value {
    Value::Object(
        t.iter()
            .filter_map(|(k, v)| item_to_json(v).map(|j| (k.to_string(), j)))
            .collect(),
    )
}

fn to_toml_value(v: &Value) -> toml_edit::Value {
    match v {
        Value::Null => toml_edit::Value::from(""),
        Value::Bool(b) => toml_edit::Value::from(*b),
        Value::Number(n) => match n.as_i64() {
            Some(i) => toml_edit::Value::from(i),
            None => toml_edit::Value::from(n.as_f64().unwrap_or(0.0)),
        },
        Value::String(s) => toml_edit::Value::from(s.as_str()),
        Value::Array(a) => {
            let mut arr = toml_edit::Array::new();
            for x in a {
                arr.push(to_toml_value(x));
            }
            toml_edit::Value::Array(arr)
        }
        Value::Object(m) => {
            let mut t = toml_edit::InlineTable::new();
            for (k, x) in m {
                t.insert(k, to_toml_value(x));
            }
            toml_edit::Value::InlineTable(t)
        }
    }
}

/// Objects under a standard table become `[a.b]` tables; deeper ones inline tables.
fn to_toml_item(v: &Value, table_ok: bool) -> toml_edit::Item {
    match v {
        Value::Object(m) if table_ok => {
            let mut t = toml_edit::Table::new();
            for (k, x) in m {
                t.insert(k, toml_edit::Item::Value(to_toml_value(x)));
            }
            toml_edit::Item::Table(t)
        }
        other => toml_edit::Item::Value(to_toml_value(other)),
    }
}

fn toml_set(table: &mut toml_edit::Table, path: &[Seg], value: &Value) -> Option<()> {
    let (last, parents) = path.split_last()?;
    let mut cur = table;
    for seg in parents {
        let Seg::Key(k) = seg else { return None };
        if !cur.contains_key(k) {
            let mut t = toml_edit::Table::new();
            t.set_implicit(true);
            cur.insert(k, toml_edit::Item::Table(t));
        }
        cur = cur.get_mut(k)?.as_table_mut()?;
    }
    let Seg::Key(k) = last else { return None };
    match cur.get_mut(k) {
        // Keep the user's decor (comments before the key) when replacing a plain value.
        Some(toml_edit::Item::Value(existing)) if !value.is_object() => {
            let decor = existing.decor().clone();
            *existing = to_toml_value(value);
            *existing.decor_mut() = decor;
        }
        _ => {
            cur.insert(k, to_toml_item(value, true));
        }
    }
    Some(())
}

fn toml_remove(table: &mut toml_edit::Table, path: &[Seg]) -> bool {
    let Some((last, parents)) = path.split_last() else {
        return false;
    };
    let mut cur = table;
    for seg in parents {
        let Seg::Key(k) = seg else { return false };
        match cur.get_mut(k).and_then(|i| i.as_table_mut()) {
            Some(t) => cur = t,
            None => return false,
        }
    }
    match last {
        Seg::Key(k) => cur.remove(k).is_some(),
        _ => false,
    }
}

// --- plain values (YAML) ---

fn value_set(root: &mut Value, path: &[Seg], value: &Value) -> Option<()> {
    let (last, parents) = path.split_last()?;
    let mut cur = root;
    for (i, seg) in parents.iter().enumerate() {
        let next = parents.get(i + 1).unwrap_or(last);
        let empty = if matches!(next, Seg::Item { .. }) {
            Value::Array(Vec::new())
        } else {
            Value::Object(Map::new())
        };
        cur = match seg {
            Seg::Key(k) => {
                let obj = cur.as_object_mut()?;
                let slot = obj.entry(k.clone()).or_insert(empty);
                if slot.is_null() {
                    *slot = if matches!(next, Seg::Item { .. }) {
                        Value::Array(Vec::new())
                    } else {
                        Value::Object(Map::new())
                    };
                }
                slot
            }
            Seg::Item { field, value: v } => {
                let arr = cur.as_array_mut()?;
                let pos = arr
                    .iter()
                    .position(|e| e.get(field).and_then(Value::as_str) == Some(v.as_str()));
                let pos = match pos {
                    Some(p) => p,
                    None => {
                        let mut m = Map::new();
                        m.insert(field.clone(), Value::String(v.clone()));
                        arr.push(Value::Object(m));
                        arr.len() - 1
                    }
                };
                &mut arr[pos]
            }
        };
        let ok = if matches!(next, Seg::Item { .. }) {
            cur.is_array()
        } else {
            cur.is_object()
        };
        if !ok {
            return None;
        }
    }
    match last {
        Seg::Key(k) => {
            cur.as_object_mut()?.insert(k.clone(), value.clone());
        }
        Seg::Item { field, value: v } => {
            let arr = cur.as_array_mut()?;
            let mut obj = value.clone();
            if let Some(m) = obj.as_object_mut() {
                m.insert(field.clone(), Value::String(v.clone()));
            }
            match arr
                .iter()
                .position(|e| e.get(field).and_then(Value::as_str) == Some(v.as_str()))
            {
                Some(p) => arr[p] = obj,
                None => arr.push(obj),
            }
        }
    }
    Some(())
}

fn value_remove(root: &mut Value, path: &[Seg]) -> bool {
    let Some((last, parents)) = path.split_last() else {
        return false;
    };
    let mut cur = root;
    for seg in parents {
        let next = match seg {
            Seg::Key(k) => cur.as_object_mut().and_then(|o| o.get_mut(k)),
            Seg::Item { field, value } => cur.as_array_mut().and_then(|a| {
                a.iter_mut()
                    .find(|e| e.get(field).and_then(Value::as_str) == Some(value.as_str()))
            }),
        };
        match next {
            Some(n) => cur = n,
            None => return false,
        }
    }
    match last {
        Seg::Key(k) => cur
            .as_object_mut()
            .and_then(|o| o.shift_remove(k))
            .is_some(),
        Seg::Item { field, value } => match cur.as_array_mut() {
            Some(a) => {
                let before = a.len();
                a.retain(|e| e.get(field).and_then(Value::as_str) != Some(value.as_str()));
                a.len() != before
            }
            None => false,
        },
    }
}

/// The value that ends up at `edit.path` (an array item also carries its name field).
fn written_value(edit: &Edit) -> Value {
    match (edit.path.last(), &edit.value) {
        (Some(Seg::Item { field, value }), Value::Object(m)) => {
            let mut m = m.clone();
            m.insert(field.clone(), Value::String(value.clone()));
            Value::Object(m)
        }
        _ => edit.value.clone(),
    }
}

// ---------------------------------------------------------------------------
// Rendering
// ---------------------------------------------------------------------------

/// The outcome of rendering one [`FilePlan`].
#[derive(Debug, Clone)]
pub struct Rendered {
    pub path: PathBuf,
    pub before: Option<String>,
    pub after: String,
    pub keys: Vec<AppliedKey>,
    pub created: bool,
    pub whole: bool,
    /// Re-serialized: comments of the original are gone (YAML).
    pub comments_lost: bool,
    /// The file changed since Harness last wrote it.
    pub drifted: bool,
}

fn read_text(path: &Path) -> Result<Option<String>, String> {
    if !path.exists() {
        return Ok(None);
    }
    let bytes = std::fs::read(path).map_err(|e| {
        err("fs.read_failed")
            .with("path", path.display())
            .with("error", e)
    })?;
    String::from_utf8(bytes)
        .map(Some)
        .map_err(|_| super::unparseable_msg(path))
}

fn yaml_has_comments(text: &str) -> bool {
    text.lines().any(|l| l.trim_start().starts_with('#'))
}

pub fn render(id: &str, plan: &FilePlan, ledger: &Ledger) -> Result<Rendered, String> {
    let path = plan.path().to_path_buf();
    let before = read_text(&path)?;
    let previous = ledger.file(id, &path);
    let drifted = match (previous, &before) {
        (Some(prev), Some(text)) => prev.sha256 != sha256_hex(text.as_bytes()),
        (Some(_), None) => true,
        _ => false,
    };
    let created = previous.map(|p| p.created).unwrap_or(before.is_none());
    match plan {
        FilePlan::Whole { content, .. } => Ok(Rendered {
            path,
            before,
            after: content.clone(),
            keys: Vec::new(),
            created,
            whole: true,
            comments_lost: false,
            drifted,
        }),
        FilePlan::Keys {
            format,
            edits,
            new_doc,
            ..
        } => {
            let mut doc = match &before {
                Some(text) if !text.trim().is_empty() => Doc::parse(*format, text, &path)?,
                _ => Doc::new(
                    *format,
                    new_doc.as_ref(),
                    edits.first().map(|e| e.path.as_slice()).unwrap_or(&[]),
                ),
            };
            let original = doc.value();
            let mut keys = Vec::new();
            for edit in edits {
                let snapshot = doc.value();
                let current = get_in(&snapshot, &edit.path).cloned();
                if let (Some(cur), Some(allow)) = (&current, edit.replace_if) {
                    if !allow(cur) {
                        continue;
                    }
                }
                let earlier = previous.and_then(|p| p.keys.iter().find(|k| k.path == edit.path));
                let (prior, created_from) = match (earlier, &current) {
                    // Ours from an earlier apply: keep what was there before Harness.
                    (Some(k), Some(cur)) if k.value_sha == value_sha(cur) => {
                        (k.prior.clone(), k.created_from)
                    }
                    _ => (current.clone(), first_missing(&snapshot, &edit.path)),
                };
                if current.as_ref() != Some(&written_value(edit)) {
                    doc.set(&edit.path, &edit.value, &path)?;
                }
                keys.push(AppliedKey {
                    path: edit.path.clone(),
                    value_sha: value_sha(&written_value(edit)),
                    urls: url_hashes(&edit.path, &written_value(edit)),
                    prior,
                    created_from,
                    marker: edit.marker,
                });
            }
            // Keys an earlier apply set that this one no longer touches stay recorded while
            // they still hold Harness's value (so a rollback still finds them).
            if let Some(prev) = previous {
                let snapshot = doc.value();
                for k in &prev.keys {
                    if keys.iter().any(|n| n.path == k.path) {
                        continue;
                    }
                    if get_in(&snapshot, &k.path).is_some_and(|v| value_sha(v) == k.value_sha) {
                        keys.push(k.clone());
                    }
                }
            }
            let after = if doc.value() == original {
                before.clone().unwrap_or(doc.text()?)
            } else {
                doc.text()?
            };
            Ok(Rendered {
                path,
                comments_lost: *format == Format::Yaml
                    && before.as_deref().is_some_and(yaml_has_comments),
                before,
                after,
                keys,
                created,
                whole: false,
                drifted,
            })
        }
    }
}

// ---------------------------------------------------------------------------
// Writing and backups
// ---------------------------------------------------------------------------

/// Local wall-clock `YYYYMMDD-HHMMSS`.
fn stamp(at: chrono::DateTime<chrono::FixedOffset>) -> String {
    at.format("%Y%m%d-%H%M%S").to_string()
}

fn now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

fn backup_prefix(path: &Path) -> Option<(PathBuf, String)> {
    let dir = path.parent()?.to_path_buf();
    let name = path.file_name()?.to_string_lossy().to_string();
    Some((dir, format!("{}.harness-", name)))
}

/// Timestamped backups of `path`, oldest first.
pub fn backups_of(path: &Path) -> Vec<PathBuf> {
    let Some((dir, prefix)) = backup_prefix(path) else {
        return Vec::new();
    };
    let mut list: Vec<PathBuf> = std::fs::read_dir(&dir)
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| e.path())
        .filter(|p| {
            p.file_name().map(|n| n.to_string_lossy()).is_some_and(|n| {
                let Some(tail) = n.strip_prefix(&prefix).and_then(|s| s.strip_suffix(".bak"))
                else {
                    return false;
                };
                let parts: Vec<_> = tail.split('-').collect();
                (parts.len() == 2 || parts.len() == 3)
                    && parts[0].len() == 8
                    && parts[1].len() == 6
                    && parts
                        .iter()
                        .all(|s| !s.is_empty() && s.bytes().all(|c| c.is_ascii_digit()))
                    && p.is_file()
            })
        })
        .collect();
    list.sort_by_key(|p| {
        let name = p.file_name().unwrap().to_string_lossy();
        let tail = name.strip_prefix(&prefix).unwrap().trim_end_matches(".bak");
        let mut parts = tail.split('-');
        let stamp = format!(
            "{}-{}",
            parts.next().unwrap_or(""),
            parts.next().unwrap_or("")
        );
        let generation = parts
            .next()
            .and_then(|s| s.parse::<u64>().ok())
            .unwrap_or(1);
        (stamp, generation)
    });
    list
}

/// The text of the newest backup of `path` whose content equals `value`.
fn backup_with_value(path: &Path, format: Format, value: &Value) -> Option<String> {
    backups_of(path).iter().rev().find_map(|b| {
        let text = read_text(b).ok()??;
        let doc = Doc::parse(format, &text, b).ok()?;
        (doc.value() == *value).then_some(text)
    })
}

/// Copies `path` to `<name>.harness-<stamp>.bak` and prunes old generations.
pub fn backup(path: &Path) -> Result<Option<PathBuf>, String> {
    backup_at(path, chrono::Local::now().fixed_offset())
}

fn backup_at(
    path: &Path,
    at: chrono::DateTime<chrono::FixedOffset>,
) -> Result<Option<PathBuf>, String> {
    if !path.is_file() {
        return Ok(None);
    }
    let (dir, prefix) = backup_prefix(path)
        .ok_or_else(|| err("integrations.invalid_config_path").with("path", path.display()))?;
    let base = format!("{}{}", prefix, stamp(at));
    let mut target = dir.join(format!("{}.bak", base));
    let mut n = backups_of(path)
        .iter()
        .filter_map(|p| {
            p.file_name()?
                .to_str()?
                .strip_prefix(&format!("{base}-"))?
                .strip_suffix(".bak")?
                .parse::<u64>()
                .ok()
        })
        .max()
        .unwrap_or(1)
        + 1;
    if n > 2 {
        target = dir.join(format!("{}-{}.bak", base, n));
        n += 1;
    }
    while target.exists() {
        target = dir.join(format!("{}-{}.bak", base, n));
        n += 1;
    }
    std::fs::copy(path, &target).map_err(|e| {
        err("integrations.backup_failed")
            .with("path", target.display())
            .with("error", e)
    })?;
    let all = backups_of(path);
    if all.len() > BACKUP_GENERATIONS {
        for old in &all[..all.len() - BACKUP_GENERATIONS] {
            let _ = std::fs::remove_file(old);
        }
    }
    Ok(Some(target))
}

fn write_file(path: &Path, text: &str) -> Result<(), String> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| {
            err("fs.create_dir_failed")
                .with("path", dir.display())
                .with("error", e)
        })?;
    }
    crate::store::JsonStore::sensitive(path)
        .write_bytes(text.as_bytes())
        .map_err(|e| {
            err("fs.write_failed")
                .with("path", path.display())
                .with("error", e)
                .into()
        })
}

/// Result of an apply.
#[derive(Debug, Clone, Default, Serialize)]
pub struct ApplyOutcome {
    pub written: Vec<String>,
    pub backups: Vec<String>,
    pub warnings: Vec<String>,
}

/// Renders every plan (nothing is written if one fails), then writes the changed files with a
/// backup and records the ledger entry.
pub fn apply(
    id: &str,
    plans: &[FilePlan],
    ledger_path: Option<&Path>,
) -> Result<ApplyOutcome, String> {
    let mut ledger = Ledger::load(ledger_path);
    let rendered = plans
        .iter()
        .map(|p| render(id, p, &ledger))
        .collect::<Result<Vec<_>, _>>()?;
    let mut out = ApplyOutcome::default();
    let mut files = Vec::new();
    for r in &rendered {
        if r.drifted {
            out.warnings.push(format!(
                "{} Harness son yazdıktan sonra değiştirilmiş; yalnız Harness'ın anahtarları güncellendi.",
                r.path.display()
            ));
        }
        if r.comments_lost {
            out.warnings.push(format!(
                "{} yeniden yazılırken yorum satırları kaldırıldı (özgün dosya yedeklendi).",
                r.path.display()
            ));
        }
        if r.before.as_deref() != Some(r.after.as_str()) {
            if let Some(b) = backup(&r.path)? {
                out.backups.push(b.to_string_lossy().to_string());
            }
            write_file(&r.path, &r.after)?;
            out.written.push(r.path.to_string_lossy().to_string());
        }
        files.push(LedgerFile {
            path: r.path.to_string_lossy().to_string(),
            sha256: sha256_hex(r.after.as_bytes()),
            created: r.created,
            whole: r.whole,
            keys: r.keys.clone(),
        });
    }
    ledger.put(LedgerEntry {
        id: id.to_string(),
        applied_at: now_secs(),
        files,
        request: None,
    });
    ledger.save(ledger_path)?;
    Ok(out)
}

/// Whether this data directory last wrote the entry containing `key_path` and it still matches.
/// Names and port history alone cannot distinguish two installations sharing a client config.
pub fn owns_key(
    id: &str,
    path: &Path,
    format: Format,
    key_path: &[Seg],
    ledger_path: Option<&Path>,
) -> bool {
    let ledger = Ledger::load(ledger_path);
    let Some(file) = ledger.file(id, path) else {
        return false;
    };
    let Ok(Some(text)) = read_text(path) else {
        return false;
    };
    if file.whole {
        return sha256_hex(text.as_bytes()) == file.sha256;
    }
    let Ok(doc) = Doc::parse(format, &text, path) else {
        return false;
    };
    let value = doc.value();
    file.keys.iter().any(|key| {
        if let Some(url) = key.urls.iter().find(|u| u.path == key_path) {
            return get_in(&value, key_path).is_some_and(|v| value_sha(v) == url.value_sha);
        }
        key.urls.is_empty()
            && key_path.starts_with(&key.path)
            && get_in(&value, &key.path).is_some_and(|v| value_sha(v) == key.value_sha)
    })
}

/// Adopts only an exact current address, without writing the client file. An unrecorded
/// address on an old port stays pending Fix; adoption alone must never move it.
pub fn adopt_url(
    id: &str,
    path: &Path,
    format: Format,
    key_path: &[Seg],
    expected: &Value,
    ledger_path: Option<&Path>,
) -> Result<(), String> {
    let mut ledger = Ledger::load(ledger_path);
    if ledger
        .file(id, path)
        .is_some_and(|f| f.whole || f.keys.iter().any(|k| key_path.starts_with(&k.path)))
    {
        return Ok(());
    }
    let Some(text) = read_text(path)? else {
        return Ok(());
    };
    let doc = Doc::parse(format, &text, path)?;
    if get_in(&doc.value(), key_path) != Some(expected) {
        return Ok(());
    }
    let key = AppliedKey {
        path: key_path.to_vec(),
        value_sha: value_sha(expected),
        urls: url_hashes(key_path, expected),
        prior: Some(expected.clone()),
        created_from: None,
        marker: false,
    };
    if ledger.entry(id).is_none() {
        ledger.put(LedgerEntry {
            id: id.to_string(),
            applied_at: 0,
            files: Vec::new(),
            request: None,
        });
    }
    let entry = ledger.entries.iter_mut().find(|e| e.id == id).unwrap();
    let name = path.to_string_lossy().to_string();
    if let Some(file) = entry.files.iter_mut().find(|f| f.path == name) {
        file.keys.push(key);
    } else {
        entry.files.push(LedgerFile {
            path: name,
            sha256: sha256_hex(text.as_bytes()),
            created: false,
            whole: false,
            keys: vec![key],
        });
    }
    ledger.save(ledger_path)
}

/// Sets one key of an existing config in place (the address' port when the gateway moved) and
/// keeps the ledger in step: the hash of every recorded key above `key_path` follows the edit
/// while that key was still as Harness wrote it (so undo still takes it out after the user
/// edited other keys of the file), the file's hash only while the whole file was. A backup is
/// kept like for any write. Returns the backup, `None` when nothing changed.
pub fn retarget(
    id: &str,
    path: &Path,
    format: Format,
    key_path: &[Seg],
    value: &Value,
    ledger_path: Option<&Path>,
) -> Result<Option<PathBuf>, String> {
    if !owns_key(id, path, format, key_path, ledger_path) {
        return Ok(None);
    }
    let text = read_text(path)?.ok_or_else(|| super::unparseable_msg(path))?;
    let mut doc = Doc::parse(format, &text, path)?;
    let before = doc.value();
    if get_in(&before, key_path) == Some(value) {
        return Ok(None);
    }
    doc.set(key_path, value, path)?;
    let after = doc.text()?;
    if after == text {
        return Ok(None);
    }
    let saved = backup(path)?;
    write_file(path, &after)?;
    let mut ledger = Ledger::load(ledger_path);
    let file = path.to_string_lossy();
    let Some(f) = ledger
        .entries
        .iter_mut()
        .find(|e| e.id == id)
        .and_then(|e| e.files.iter_mut().find(|f| f.path == file))
    else {
        return Ok(saved);
    };
    let now = doc.value();
    for k in f.keys.iter_mut().filter(|k| key_path.starts_with(&k.path)) {
        for url in k.urls.iter_mut().filter(|u| u.path == key_path) {
            url.value_sha = value_sha(value);
        }
        if get_in(&before, &k.path).is_some_and(|v| value_sha(v) == k.value_sha) {
            if let Some(v) = get_in(&now, &k.path) {
                k.value_sha = value_sha(v);
            }
        }
    }
    if f.sha256 == sha256_hex(text.as_bytes()) {
        f.sha256 = sha256_hex(after.as_bytes());
    }
    ledger.save(ledger_path)?;
    Ok(saved)
}

/// Result of a rollback.
#[derive(Debug, Clone, Default, Serialize)]
pub struct RevertOutcome {
    pub written: Vec<String>,
    pub deleted: Vec<String>,
    pub backups: Vec<String>,
    /// Keys the user changed after apply (left as they are), drift notes.
    pub warnings: Vec<String>,
}

fn only_schema(v: &Value) -> bool {
    v.as_object()
        .is_some_and(|m| m.keys().all(|k| k == "$schema"))
}

/// Removes (or restores) the keys Harness set. `markers` is used when there is no ledger
/// entry: only keys that name Harness are removed.
pub fn revert(
    id: &str,
    markers: &[(PathBuf, Format, Vec<Vec<Seg>>)],
    ledger_path: Option<&Path>,
) -> Result<RevertOutcome, String> {
    let mut ledger = Ledger::load(ledger_path);
    let files: Vec<LedgerFile> = match ledger.entry(id) {
        Some(e) => e.files.clone(),
        None => markers
            .iter()
            .filter(|(p, _, _)| p.is_file())
            .map(|(p, _, paths)| LedgerFile {
                path: p.to_string_lossy().to_string(),
                sha256: String::new(),
                created: false,
                whole: false,
                keys: paths
                    .iter()
                    .map(|path| AppliedKey {
                        path: path.clone(),
                        value_sha: String::new(),
                        urls: Vec::new(),
                        prior: None,
                        created_from: None,
                        marker: true,
                    })
                    .collect(),
            })
            .collect(),
    };
    let format_of = |p: &Path| -> Format {
        markers
            .iter()
            .find(|(mp, _, _)| mp == p)
            .map(|(_, f, _)| *f)
            .unwrap_or_else(|| guess_format(p))
    };
    let mut out = RevertOutcome::default();
    let mut pending: Vec<(PathBuf, Option<String>)> = Vec::new();
    let mut found_any = false;
    for f in &files {
        let path = PathBuf::from(&f.path);
        let Some(text) = read_text(&path)? else {
            continue;
        };
        let drifted = !f.sha256.is_empty() && f.sha256 != sha256_hex(text.as_bytes());
        if f.whole {
            if drifted {
                out.warnings.push(format!(
                    "{} Harness yazdıktan sonra değiştirilmiş; dosya silinmedi.",
                    path.display()
                ));
            } else {
                found_any = true;
                pending.push((path, None));
            }
            continue;
        }
        if drifted {
            out.warnings.push(format!(
                "{} Harness yazdıktan sonra değiştirilmiş; yalnız Harness'ın hâlâ aynı olan anahtarları geri alınıyor.",
                path.display()
            ));
        }
        let format = format_of(&path);
        let mut doc = Doc::parse(format, &text, &path)?;
        let mut changed = false;
        let mut kept_user_change = false;
        for k in &f.keys {
            let snapshot = doc.value();
            let Some(cur) = get_in(&snapshot, &k.path).cloned() else {
                continue;
            };
            let ours = if k.value_sha.is_empty() {
                k.marker
            } else {
                value_sha(&cur) == k.value_sha
            };
            if !ours {
                kept_user_change = true;
                out.warnings.push(format!(
                    "{}: \"{}\" Harness'tan sonra değiştirilmiş, olduğu gibi bırakıldı.",
                    path.display(),
                    path_text(&k.path)
                ));
                continue;
            }
            found_any = true;
            if k.prior.as_ref() == Some(&cur) {
                continue;
            }
            changed = true;
            match &k.prior {
                Some(prior) => doc.set(&k.path, prior, &path)?,
                None => {
                    doc.remove(&k.path);
                    // Parents Harness created and that are now empty go too.
                    if let Some(from) = k.created_from {
                        for depth in (from..k.path.len() - 1).rev() {
                            let snap = doc.value();
                            match get_in(&snap, &k.path[..=depth]) {
                                Some(v) if is_empty_container(v) => {
                                    doc.remove(&k.path[..=depth]);
                                }
                                _ => break,
                            }
                        }
                    }
                }
            }
        }
        if !changed {
            continue;
        }
        let value = doc.value();
        if f.created && (is_empty_container(&value) || only_schema(&value)) {
            pending.push((path, None));
        } else {
            let rendered = doc.text()?;
            // Untouched since Harness wrote it and every key was Harness's: when a backup holds
            // exactly this content, its bytes come back (an edit can reflow formatting the
            // removal does not undo, e.g. an inline object that had to grow).
            let exact = (!f.sha256.is_empty() && !drifted && !kept_user_change)
                .then(|| backup_with_value(&path, format, &value))
                .flatten();
            pending.push((path, Some(exact.unwrap_or(rendered))));
        }
    }
    if !found_any {
        ledger.remove(id);
        ledger.save(ledger_path)?;
        if out.warnings.is_empty() {
            return Err(err("integrations.nothing_to_revert").into());
        }
        return Ok(out);
    }
    for (path, text) in pending {
        if let Some(b) = backup(&path)? {
            out.backups.push(b.to_string_lossy().to_string());
        }
        match text {
            Some(t) => {
                write_file(&path, &t)?;
                out.written.push(path.to_string_lossy().to_string());
            }
            None => {
                std::fs::remove_file(&path).map_err(|e| {
                    err("fs.remove_failed")
                        .with("path", path.display())
                        .with("error", e)
                })?;
                out.deleted.push(path.to_string_lossy().to_string());
            }
        }
    }
    ledger.remove(id);
    ledger.save(ledger_path)?;
    Ok(out)
}

fn guess_format(p: &Path) -> Format {
    match p
        .extension()
        .map(|e| e.to_string_lossy().to_ascii_lowercase())
        .as_deref()
    {
        Some("toml") => Format::Toml,
        Some("yaml") | Some("yml") => Format::Yaml,
        _ => Format::Json,
    }
}

/// Files of an integration whose content no longer matches what Harness wrote.
pub fn drifted_files(id: &str, ledger_path: Option<&Path>) -> Vec<String> {
    let ledger = Ledger::load(ledger_path);
    let Some(entry) = ledger.entry(id) else {
        return Vec::new();
    };
    entry
        .files
        .iter()
        .filter(|f| match std::fs::read(&f.path) {
            Ok(bytes) => sha256_hex(&bytes) != f.sha256,
            Err(_) => true,
        })
        .map(|f| f.path.clone())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use tempfile::tempdir;

    fn plan_json(path: &Path, edits: Vec<Edit>) -> FilePlan {
        FilePlan::Keys {
            path: path.to_path_buf(),
            format: Format::Json,
            edits,
            new_doc: None,
        }
    }

    #[test]
    fn stamp_formats_local_wall_clock() {
        for (at, expected) in [
            ("2026-10-08T17:00:49+03:00", "20261008-170049"),
            ("2026-10-08T07:00:49-07:00", "20261008-070049"),
            ("2027-01-01T00:00:01+03:00", "20270101-000001"),
        ] {
            assert_eq!(
                stamp(chrono::DateTime::parse_from_rfc3339(at).unwrap()),
                expected
            );
        }
        let local = chrono::Local::now();
        assert_eq!(
            stamp(local.fixed_offset()),
            local.format("%Y%m%d-%H%M%S").to_string()
        );
    }

    #[test]
    fn backup_uses_local_stamp_and_preserves_same_second_collisions() {
        let d = tempdir().unwrap();
        let file = d.path().join("settings.json");
        let at = chrono::DateTime::parse_from_rfc3339("2026-10-08T17:00:49+03:00").unwrap();
        std::fs::write(&file, "first").unwrap();
        let first = backup_at(&file, at).unwrap().unwrap();
        std::fs::write(&file, "second").unwrap();
        let second = backup_at(&file, at).unwrap().unwrap();
        assert_eq!(
            first.file_name().unwrap(),
            "settings.json.harness-20261008-170049.bak"
        );
        assert_eq!(
            second.file_name().unwrap(),
            "settings.json.harness-20261008-170049-2.bak"
        );
        assert_eq!(std::fs::read_to_string(first).unwrap(), "first");
        assert_eq!(std::fs::read_to_string(second).unwrap(), "second");
    }

    #[test]
    fn jsonc_comments_survive_apply_and_revert_restores_prior_values() {
        let d = tempdir().unwrap();
        let ledger = d.path().join("ledger.json");
        let file = d.path().join("settings.json");
        let original = "// my settings\n{\n  // keep me\n  \"theme\": \"dark\",\n  \"env\": {\n    \"FOO\": \"1\",\n    \"ANTHROPIC_MODEL\": \"mine\"\n  },\n}\n";
        std::fs::write(&file, original).unwrap();
        let plans = vec![plan_json(
            &file,
            vec![
                Edit::set(keys(&["env", "ANTHROPIC_BASE_URL"]), json!("http://x")),
                Edit::set(keys(&["env", "ANTHROPIC_MODEL"]), json!("qwen")),
            ],
        )];
        let out = apply("t", &plans, Some(&ledger)).unwrap();
        assert_eq!(out.written.len(), 1);
        let text = std::fs::read_to_string(&file).unwrap();
        assert!(text.contains("// my settings") && text.contains("// keep me"));
        assert!(text.contains("\"ANTHROPIC_BASE_URL\": \"http://x\""));
        assert_eq!(backups_of(&file).len(), 1);
        // Idempotent: a second apply writes nothing and keeps the original prior.
        let again = apply("t", &plans, Some(&ledger)).unwrap();
        assert!(again.written.is_empty());
        let rev = revert("t", &[], Some(&ledger)).unwrap();
        assert!(rev.warnings.is_empty(), "{:?}", rev.warnings);
        let back = std::fs::read_to_string(&file).unwrap();
        let v: Value = super::super::parse_tolerant(&back).unwrap().0;
        assert_eq!(v["env"]["ANTHROPIC_MODEL"], "mine");
        assert_eq!(v["env"]["FOO"], "1");
        assert!(v["env"].get("ANTHROPIC_BASE_URL").is_none());
        assert!(back.contains("// keep me"));
        assert!(Ledger::load(Some(&ledger)).entry("t").is_none());
    }

    #[test]
    fn clean_revert_brings_back_the_exact_bytes_of_an_inline_object() {
        let d = tempdir().unwrap();
        let ledger = d.path().join("ledger.json");
        let file = d.path().join("mcp.json");
        // Adding a key to an inline object reflows it over several lines.
        let original = "{
    // mine
    \"mcpServers\": { \"other\": { \"url\": \"u\" } }
}
";
        std::fs::write(&file, original).unwrap();
        let plans = vec![plan_json(
            &file,
            vec![Edit::marker(
                keys(&["mcpServers", "h"]),
                json!({"url": "x"}),
            )],
        )];
        apply("t", &plans, Some(&ledger)).unwrap();
        assert_ne!(std::fs::read_to_string(&file).unwrap(), original);
        revert("t", &[], Some(&ledger)).unwrap();
        assert_eq!(std::fs::read_to_string(&file).unwrap(), original);

        // Edited after apply (a comment added): the rendered removal keeps the user's edit
        // instead of an older backup.
        apply("t", &plans, Some(&ledger)).unwrap();
        let edited = std::fs::read_to_string(&file)
            .unwrap()
            .replace("// mine", "// mine, edited");
        std::fs::write(&file, &edited).unwrap();
        revert("t", &[], Some(&ledger)).unwrap();
        let after = std::fs::read_to_string(&file).unwrap();
        assert!(after.contains("// mine, edited"));
        assert!(!after.contains("\"h\""));
    }

    #[test]
    fn user_changes_after_apply_are_kept_and_reported() {
        let d = tempdir().unwrap();
        let ledger = d.path().join("ledger.json");
        let file = d.path().join("c.json");
        std::fs::write(&file, "{\"a\": 1}").unwrap();
        let plans = vec![plan_json(
            &file,
            vec![
                Edit::set(keys(&["x", "url"]), json!("u")),
                Edit::set(keys(&["x", "model"]), json!("m")),
            ],
        )];
        apply("t", &plans, Some(&ledger)).unwrap();
        // The user edits one of our keys.
        let text = std::fs::read_to_string(&file)
            .unwrap()
            .replace("\"m\"", "\"theirs\"");
        std::fs::write(&file, text).unwrap();
        assert_eq!(drifted_files("t", Some(&ledger)).len(), 1);
        let rev = revert("t", &[], Some(&ledger)).unwrap();
        assert_eq!(rev.warnings.len(), 2, "{:?}", rev.warnings); // drift + changed key
        let v: Value = serde_json::from_str(&std::fs::read_to_string(&file).unwrap()).unwrap();
        assert_eq!(v["x"]["model"], "theirs");
        assert!(v["x"].get("url").is_none());
        assert_eq!(v["a"], 1);
    }

    #[test]
    fn created_parents_and_files_are_removed_again() {
        let d = tempdir().unwrap();
        let ledger = d.path().join("ledger.json");
        let file = d.path().join("new").join("f.json");
        let plans = vec![plan_json(
            &file,
            vec![Edit::set(keys(&["a", "b", "c"]), json!(1))],
        )];
        apply("t", &plans, Some(&ledger)).unwrap();
        assert!(file.exists());
        let rev = revert("t", &[], Some(&ledger)).unwrap();
        assert_eq!(rev.deleted.len(), 1);
        assert!(!file.exists());

        // Existing file: the created parent object is removed, the user's keys stay.
        std::fs::write(&file, "{\"keep\": true}").unwrap();
        apply("t", &plans, Some(&ledger)).unwrap();
        revert("t", &[], Some(&ledger)).unwrap();
        let v: Value = serde_json::from_str(&std::fs::read_to_string(&file).unwrap()).unwrap();
        assert_eq!(v, json!({"keep": true}));
    }

    #[test]
    fn array_items_by_name() {
        let d = tempdir().unwrap();
        let ledger = d.path().join("ledger.json");
        let file = d.path().join("c.json");
        std::fs::write(&file, "{\"models\": [{\"title\": \"Mine\"}]}").unwrap();
        let path = vec![Seg::key("models"), Seg::item("title", "Harness")];
        let plans = vec![plan_json(
            &file,
            vec![Edit::marker(path.clone(), json!({"model": "a"}))],
        )];
        apply("t", &plans, Some(&ledger)).unwrap();
        let v: Value = serde_json::from_str(&std::fs::read_to_string(&file).unwrap()).unwrap();
        assert_eq!(v["models"][1], json!({"model": "a", "title": "Harness"}));
        // Update in place.
        let plans2 = vec![plan_json(
            &file,
            vec![Edit::marker(path.clone(), json!({"model": "b"}))],
        )];
        apply("t", &plans2, Some(&ledger)).unwrap();
        let v: Value = serde_json::from_str(&std::fs::read_to_string(&file).unwrap()).unwrap();
        assert_eq!(v["models"].as_array().unwrap().len(), 2);
        assert_eq!(v["models"][1]["model"], "b");
        revert("t", &[], Some(&ledger)).unwrap();
        let v: Value = serde_json::from_str(&std::fs::read_to_string(&file).unwrap()).unwrap();
        assert_eq!(v, json!({"models": [{"title": "Mine"}]}));
    }

    #[test]
    fn toml_keeps_comments_and_reverts_table() {
        let d = tempdir().unwrap();
        let ledger = d.path().join("ledger.json");
        let file = d.path().join("config.toml");
        let original = "# my codex\nmodel = \"gpt-5\"\n\n[mcp_servers.x]\ncommand = \"x\"\n";
        std::fs::write(&file, original).unwrap();
        let plans = vec![FilePlan::Keys {
            path: file.clone(),
            format: Format::Toml,
            edits: vec![Edit::marker(
                keys(&["model_providers", "harness"]),
                json!({"name": "Harness Studio", "base_url": "http://h/v1", "wire_api": "responses"}),
            )],
            new_doc: None,
        }];
        apply("t", &plans, Some(&ledger)).unwrap();
        let text = std::fs::read_to_string(&file).unwrap();
        assert!(text.contains("# my codex"));
        assert!(text.contains("[model_providers.harness]"), "{text}");
        let parsed: toml::Value = toml::from_str(&text).unwrap();
        assert_eq!(
            parsed["model_providers"]["harness"]["wire_api"].as_str(),
            Some("responses")
        );
        revert("t", &[], Some(&ledger)).unwrap();
        let back = std::fs::read_to_string(&file).unwrap();
        let parsed: toml::Value = toml::from_str(&back).unwrap();
        assert!(parsed.get("model_providers").is_none(), "{back}");
        assert!(back.contains("# my codex") && back.contains("[mcp_servers.x]"));
    }

    #[test]
    fn marker_fallback_without_ledger_removes_only_markers() {
        let d = tempdir().unwrap();
        let file = d.path().join("opencode.json");
        std::fs::write(
            &file,
            "{\"provider\": {\"harness\": {}, \"other\": {}}, \"model\": \"x/y\"}",
        )
        .unwrap();
        let markers = vec![(
            file.clone(),
            Format::Json,
            vec![keys(&["provider", "harness"])],
        )];
        let ledger = d.path().join("ledger.json");
        revert("t", &markers, Some(&ledger)).unwrap();
        let v: Value = serde_json::from_str(&std::fs::read_to_string(&file).unwrap()).unwrap();
        assert_eq!(v, json!({"provider": {"other": {}}, "model": "x/y"}));
        assert!(revert("t", &markers, Some(&ledger)).is_err());
    }

    #[test]
    fn url_ownership_survives_other_block_edits_in_every_format() {
        for (format, ext) in [
            (Format::Json, "json"),
            (Format::Toml, "toml"),
            (Format::Yaml, "yaml"),
        ] {
            let d = tempdir().unwrap();
            let path = d.path().join(format!("settings.{ext}"));
            let ledger = d.path().join("ledger.json");
            for field in ["baseURL", "base_url", "apiBase", "api_url", "url"] {
                let plan = FilePlan::Keys {
                    path: path.clone(),
                    format,
                    edits: vec![Edit::marker(
                        keys(&["provider"]),
                        serde_json::json!({
                            field: "http://127.0.0.1:12001/v1", "models": {}, "apiKey": "placeholder"
                        }),
                    )],
                    new_doc: None,
                };
                apply("t", &[plan], Some(&ledger)).unwrap();
                let mut doc =
                    Doc::parse(format, &read_text(&path).unwrap().unwrap(), &path).unwrap();
                doc.set(
                    &keys(&["provider", "apiKey"]),
                    &serde_json::json!("user-key"),
                    &path,
                )
                .unwrap();
                doc.set(
                    &keys(&["provider", "models"]),
                    &serde_json::json!({"extra": {"name": "Extra"}}),
                    &path,
                )
                .unwrap();
                std::fs::write(&path, doc.text().unwrap()).unwrap();
                let url = keys(&["provider", field]);
                assert!(owns_key("t", &path, format, &url, Some(&ledger)));
                retarget(
                    "t",
                    &path,
                    format,
                    &url,
                    &serde_json::json!("http://127.0.0.1:12002/v1"),
                    Some(&ledger),
                )
                .unwrap();
                assert!(owns_key("t", &path, format, &url, Some(&ledger)));
                let text = read_text(&path).unwrap().unwrap();
                assert!(text.contains("user-key") && text.contains("Extra"));
                std::fs::write(&path, text.replace(":12002", ":12003")).unwrap();
                assert!(!owns_key("t", &path, format, &url, Some(&ledger)));
            }
        }
    }

    #[test]
    fn legacy_url_ownership_requires_the_original_block_hash() {
        let d = tempdir().unwrap();
        let path = d.path().join("settings.json");
        let ledger_path = d.path().join("ledger.json");
        let plan = FilePlan::Keys {
            path: path.clone(),
            format: Format::Json,
            edits: vec![Edit::marker(
                keys(&["provider"]),
                serde_json::json!({
                    "baseURL": "http://127.0.0.1:12001/v1", "apiKey": "placeholder"
                }),
            )],
            new_doc: None,
        };
        apply("t", &[plan], Some(&ledger_path)).unwrap();
        let mut ledger = serde_json::to_value(Ledger::load(Some(&ledger_path))).unwrap();
        ledger["entries"][0]["files"][0]["keys"][0]
            .as_object_mut()
            .unwrap()
            .remove("urls");
        std::fs::write(&ledger_path, serde_json::to_string(&ledger).unwrap()).unwrap();
        let url = keys(&["provider", "baseURL"]);
        assert!(owns_key("t", &path, Format::Json, &url, Some(&ledger_path)));
        let text = std::fs::read_to_string(&path)
            .unwrap()
            .replace("placeholder", "user-key");
        std::fs::write(&path, &text).unwrap();
        assert!(!owns_key(
            "t",
            &path,
            Format::Json,
            &url,
            Some(&ledger_path)
        ));
        assert!(retarget(
            "t",
            &path,
            Format::Json,
            &url,
            &serde_json::json!("http://127.0.0.1:12002/v1"),
            Some(&ledger_path)
        )
        .unwrap()
        .is_none());
        assert_eq!(std::fs::read_to_string(&path).unwrap(), text);
    }

    #[test]
    fn named_array_url_hashes_ignore_other_fields_and_new_entries() {
        let d = tempdir().unwrap();
        let path = d.path().join("settings.json");
        let ledger = d.path().join("ledger.json");
        let entry = vec![Seg::key("models"), Seg::item("title", "Harness")];
        let plan = FilePlan::Keys {
            path: path.clone(),
            format: Format::Json,
            edits: vec![Edit::marker(
                entry.clone(),
                serde_json::json!({
                    "apiBase": "http://127.0.0.1:12001/v1", "apiKey": "placeholder"
                }),
            )],
            new_doc: None,
        };
        apply("t", &[plan], Some(&ledger)).unwrap();
        let mut doc = Doc::parse(Format::Json, &read_text(&path).unwrap().unwrap(), &path).unwrap();
        let mut key = entry.clone();
        key.push(Seg::key("apiKey"));
        doc.set(&key, &serde_json::json!("user-key"), &path)
            .unwrap();
        std::fs::write(&path, doc.text().unwrap()).unwrap();
        let mut url = entry;
        url.push(Seg::key("apiBase"));
        assert!(owns_key("t", &path, Format::Json, &url, Some(&ledger)));
        assert!(retarget(
            "t",
            &path,
            Format::Json,
            &url,
            &serde_json::json!("http://127.0.0.1:12002/v1"),
            Some(&ledger)
        )
        .unwrap()
        .is_some());
        assert!(std::fs::read_to_string(&path).unwrap().contains("user-key"));
        assert!(!owns_key(
            "t",
            &path,
            Format::Json,
            &[
                Seg::key("models"),
                Seg::item("title", "User"),
                Seg::key("apiBase")
            ],
            Some(&ledger)
        ));
    }

    #[test]
    fn noop_takeover_disconnect_preserves_the_existing_file() {
        let d = tempdir().unwrap();
        let path = d.path().join("settings.json");
        let owner = d.path().join("owner.json");
        let isolated = d.path().join("isolated.json");
        let plan = FilePlan::Keys {
            path: path.clone(),
            format: Format::Json,
            edits: vec![Edit::marker(
                keys(&["provider"]),
                serde_json::json!({"url": "http://127.0.0.1:12001/v1"}),
            )],
            new_doc: None,
        };
        apply("t", std::slice::from_ref(&plan), Some(&owner)).unwrap();
        let before = std::fs::read(&path).unwrap();
        let backups = backups_of(&path);
        assert!(apply("t", &[plan], Some(&isolated))
            .unwrap()
            .written
            .is_empty());
        let markers = vec![(path.clone(), Format::Json, vec![keys(&["provider"])])];
        let out = revert("t", &markers, Some(&isolated)).unwrap();
        assert!(out.written.is_empty() && out.deleted.is_empty() && out.backups.is_empty());
        assert_eq!(std::fs::read(&path).unwrap(), before);
        assert_eq!(backups_of(&path), backups);
        assert!(Ledger::load(Some(&isolated)).entry("t").is_none());
        assert!(owns_key(
            "t",
            &path,
            Format::Json,
            &keys(&["provider", "url"]),
            Some(&owner)
        ));
    }

    #[test]
    fn repeated_writes_prune_backups_and_noops_preserve_bytes() {
        let d = tempdir().unwrap();
        let file = d.path().join("settings.json");
        let ledger = d.path().join("ledger.json");
        let at = chrono::DateTime::parse_from_rfc3339("2026-10-08T17:00:49+03:00").unwrap();
        let user_backup = d.path().join("settings.json.harness-old.bak");
        std::fs::write(&user_backup, "mine").unwrap();
        for name in [
            "20261008-170049-extra",
            "2026108-170049",
            "20261008-170049-",
            "20261008-170049-2-3",
        ] {
            std::fs::write(
                d.path().join(format!("settings.json.harness-{name}.bak")),
                "mine",
            )
            .unwrap();
        }
        for i in 0..20 {
            std::fs::write(&file, i.to_string()).unwrap();
            let latest = backup_at(&file, at).unwrap().unwrap();
            assert_eq!(std::fs::read_to_string(latest).unwrap(), i.to_string());
            assert!(backups_of(&file).len() <= BACKUP_GENERATIONS);
        }
        let values: Vec<_> = backups_of(&file)
            .iter()
            .map(|p| {
                std::fs::read_to_string(p)
                    .unwrap()
                    .parse::<usize>()
                    .unwrap()
            })
            .collect();
        assert_eq!(values, (15..20).collect::<Vec<_>>());
        assert_eq!(std::fs::read_to_string(user_backup).unwrap(), "mine");

        std::fs::write(&file, "{ \"value\": 42 }").unwrap();
        let plan = FilePlan::Keys {
            path: file.clone(),
            format: Format::Json,
            edits: vec![Edit::set(keys(&["value"]), serde_json::json!(42))],
            new_doc: None,
        };
        let before = std::fs::read(&file).unwrap();
        let backups = backups_of(&file);
        let out = apply("test", &[plan], Some(&ledger)).unwrap();
        assert!(out.written.is_empty() && out.backups.is_empty());
        assert_eq!(std::fs::read(&file).unwrap(), before);
        assert_eq!(backups_of(&file), backups);
        assert!(retarget(
            "test",
            &file,
            Format::Json,
            &keys(&["value"]),
            &serde_json::json!(42),
            Some(&ledger)
        )
        .unwrap()
        .is_none());
        assert_eq!(std::fs::read(&file).unwrap(), before);
        assert_eq!(backups_of(&file), backups);
    }

    #[test]
    fn whole_files_and_backup_generations() {
        let d = tempdir().unwrap();
        let ledger = d.path().join("ledger.json");
        let file = d.path().join("p.json");
        for i in 0..(BACKUP_GENERATIONS + 3) {
            std::fs::write(&file, format!("{{\"i\": {}}}", i)).unwrap();
            backup(&file).unwrap();
        }
        assert_eq!(backups_of(&file).len(), BACKUP_GENERATIONS);
        let owned = d.path().join("owned.json");
        let plans = vec![FilePlan::Whole {
            path: owned.clone(),
            content: "{}\n".into(),
        }];
        apply("w", &plans, Some(&ledger)).unwrap();
        let rev = revert("w", &[], Some(&ledger)).unwrap();
        assert_eq!(rev.deleted.len(), 1);
        assert!(!owned.exists());
        // A whole file the user edited is kept.
        apply("w", &plans, Some(&ledger)).unwrap();
        std::fs::write(&owned, "{\"mine\":1}").unwrap();
        let rev = revert("w", &[], Some(&ledger)).unwrap();
        assert!(owned.exists());
        assert_eq!(rev.warnings.len(), 1);
    }

    #[test]
    fn broken_file_is_never_touched() {
        let d = tempdir().unwrap();
        let ledger = d.path().join("ledger.json");
        let good = d.path().join("good.json");
        let bad = d.path().join("bad.json");
        std::fs::write(&bad, "{ oops").unwrap();
        let plans = vec![
            plan_json(&good, vec![Edit::set(keys(&["a"]), json!(1))]),
            plan_json(&bad, vec![Edit::set(keys(&["a"]), json!(1))]),
        ];
        assert!(apply("t", &plans, Some(&ledger)).is_err());
        assert!(!good.exists());
        assert_eq!(std::fs::read_to_string(&bad).unwrap(), "{ oops");
    }

    #[test]
    fn root_array_documents() {
        let d = tempdir().unwrap();
        let ledger = d.path().join("ledger.json");
        let file = d.path().join("chatLanguageModels.json");
        let path = vec![Seg::item("name", "Harness Studio")];
        let plans = vec![plan_json(
            &file,
            vec![Edit::marker(
                path.clone(),
                json!({"vendor": "customendpoint"}),
            )],
        )];
        apply("v", &plans, Some(&ledger)).unwrap();
        let v: Value = serde_json::from_str(&std::fs::read_to_string(&file).unwrap()).unwrap();
        assert_eq!(
            v,
            json!([{"vendor": "customendpoint", "name": "Harness Studio"}])
        );
        let rev = revert("v", &[], Some(&ledger)).unwrap();
        assert_eq!(rev.deleted.len(), 1);
        std::fs::write(&file, "[{\"name\": \"Mine\", \"vendor\": \"x\"}]").unwrap();
        apply("v", &plans, Some(&ledger)).unwrap();
        revert("v", &[], Some(&ledger)).unwrap();
        let v: Value = serde_json::from_str(&std::fs::read_to_string(&file).unwrap()).unwrap();
        assert_eq!(v, json!([{"name": "Mine", "vendor": "x"}]));
    }

    #[test]
    fn yaml_items() {
        let d = tempdir().unwrap();
        let ledger = d.path().join("ledger.json");
        let file = d.path().join("librechat.yaml");
        std::fs::write(
            &file,
            "# c\nversion: 1.2.1\nendpoints:\n  custom:\n    - name: Other\n      baseURL: x\n",
        )
        .unwrap();
        let path = vec![
            Seg::key("endpoints"),
            Seg::key("custom"),
            Seg::item("name", "Harness Studio"),
        ];
        let plans = vec![FilePlan::Keys {
            path: file.clone(),
            format: Format::Yaml,
            edits: vec![Edit::marker(path, json!({"baseURL": "http://h/v1"}))],
            new_doc: None,
        }];
        let out = apply("y", &plans, Some(&ledger)).unwrap();
        assert_eq!(out.warnings.len(), 1); // comments lost
        let v = super::super::parse_yaml(&std::fs::read_to_string(&file).unwrap()).unwrap();
        assert_eq!(v["endpoints"]["custom"].as_array().unwrap().len(), 2);
        revert("y", &[], Some(&ledger)).unwrap();
        let v = super::super::parse_yaml(&std::fs::read_to_string(&file).unwrap()).unwrap();
        assert_eq!(v["endpoints"]["custom"].as_array().unwrap().len(), 1);
        assert_eq!(v["version"], "1.2.1");
    }
}
