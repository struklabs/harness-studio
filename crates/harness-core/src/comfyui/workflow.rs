//! ComfyUI workflow graphs: the API ("prompt") format ComfyUI runs, the editor's UI format
//! converted to it with the server's node definitions (`/object_info`), a built-in
//! text-to-image graph, and the prompt / seed / size parameters written into a graph.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};

use crate::error_key::err;

/// API format: node id -> `{class_type, inputs, _meta}`; a linked input is `[node id, slot]`.
pub type Graph = Map<String, Value>;

/// Smallest and largest picture side, in pixels (multiples of 8).
pub const SIDE_RANGE: std::ops::RangeInclusive<u32> = 64..=8192;
/// Longest prompt passed on.
pub const MAX_PROMPT_CHARS: usize = 8000;
/// Prefix of the files ComfyUI writes for the built-in graph (`output/HarnessStudio_00001_.png`).
pub const FILENAME_PREFIX: &str = "HarnessStudio";

/// Text inputs of the prompt encoders (`CLIPTextEncode`, `CLIPTextEncodeSDXL`, Flux...).
const TEXT_INPUTS: &[&str] = &["text", "text_g", "text_l", "clip_l", "t5xxl"];
/// Editor-only nodes that are not part of the graph ComfyUI runs.
const VIRTUAL_NODES: &[&str] = &["Note", "MarkdownNote", "Reroute", "PrimitiveNode"];
/// Widget kinds of `/object_info` input specs (anything else is a link-only input).
const WIDGET_TYPES: &[&str] = &["INT", "FLOAT", "STRING", "BOOLEAN", "COMBO"];
/// Node modes of the editor: 2 = muted (never runs), 4 = bypassed (inputs pass through).
const MODE_MUTED: i64 = 2;
const MODE_BYPASS: i64 = 4;
/// Longest chain of reroutes and bypassed nodes followed for one link.
const MAX_LINK_HOPS: usize = 64;

/// What the Playground sets on a text-to-image run. `None` keeps the workflow's own value.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Txt2ImgParams {
    pub prompt: String,
    /// Empty keeps the workflow's own negative prompt.
    pub negative: String,
    /// `None` = a random seed (the one used comes back with the result).
    pub seed: Option<u64>,
    pub width: Option<u32>,
    pub height: Option<u32>,
    pub steps: Option<u32>,
    pub cfg: Option<f64>,
    /// Checkpoint file for the built-in graph (`CheckpointLoaderSimple`).
    pub checkpoint: Option<String>,
}

impl Txt2ImgParams {
    pub fn check(&self) -> Result<(), String> {
        let prompt = self.prompt.trim();
        if prompt.is_empty() {
            return Err(err("comfyui.prompt_empty").into());
        }
        if prompt.chars().count() > MAX_PROMPT_CHARS
            || self.negative.chars().count() > MAX_PROMPT_CHARS
        {
            return Err(err("comfyui.prompt_too_long")
                .num("max", MAX_PROMPT_CHARS)
                .into());
        }
        for side in [self.width, self.height].into_iter().flatten() {
            if !SIDE_RANGE.contains(&side) || side % 8 != 0 {
                return Err(err("comfyui.size_invalid").into());
            }
        }
        if self.steps.is_some_and(|s| !(1..=200).contains(&s)) {
            return Err(err("comfyui.steps_invalid").into());
        }
        if self.cfg.is_some_and(|c| !(0.0..=100.0).contains(&c)) {
            return Err(err("comfyui.cfg_invalid").into());
        }
        Ok(())
    }
}

/// Whether a workflow file is already in API format (every entry is a node with a class).
pub fn is_api_format(v: &Value) -> bool {
    v.as_object().is_some_and(|m| {
        !m.is_empty()
            && m.values()
                .all(|n| n.get("class_type").and_then(Value::as_str).is_some())
    })
}

/// Whether a workflow file is in the editor's UI format (`nodes` + `links`).
pub fn is_ui_format(v: &Value) -> bool {
    v.get("nodes").is_some_and(Value::is_array)
}

/// The classic text-to-image graph: checkpoint -> two prompts -> KSampler -> VAE decode -> save.
pub fn builtin_txt2img(checkpoint: &str) -> Graph {
    let graph = json!({
        "4": {"class_type": "CheckpointLoaderSimple", "inputs": {"ckpt_name": checkpoint}},
        "5": {"class_type": "EmptyLatentImage", "inputs": {"width": 512, "height": 512, "batch_size": 1}},
        "6": {"class_type": "CLIPTextEncode", "inputs": {"text": "", "clip": ["4", 1]}},
        "7": {"class_type": "CLIPTextEncode", "inputs": {"text": "", "clip": ["4", 1]}},
        "3": {"class_type": "KSampler", "inputs": {
            "seed": 0, "steps": 20, "cfg": 7.0, "sampler_name": "euler", "scheduler": "normal",
            "denoise": 1.0, "model": ["4", 0], "positive": ["6", 0], "negative": ["7", 0],
            "latent_image": ["5", 0]
        }},
        "8": {"class_type": "VAEDecode", "inputs": {"samples": ["3", 0], "vae": ["4", 2]}},
        "9": {"class_type": "SaveImage", "inputs": {"filename_prefix": FILENAME_PREFIX, "images": ["8", 0]}}
    });
    match graph {
        Value::Object(m) => m,
        _ => unreachable!("literal object"),
    }
}

/// Converts an editor (UI format) workflow to the API format with the server's node
/// definitions. Plain nodes, reroutes, primitive widgets, muted and bypassed nodes are
/// handled; subgraphs and group nodes are not (the user can export the API format instead).
pub fn ui_to_api(ui: &Value, object_info: &Value) -> Result<Graph, String> {
    let nodes = ui
        .get("nodes")
        .and_then(Value::as_array)
        .ok_or_else(|| String::from(err("comfyui.workflow_invalid")))?;
    if ui
        .pointer("/definitions/subgraphs")
        .and_then(Value::as_array)
        .is_some_and(|s| !s.is_empty())
        || ui
            .pointer("/extra/groupNodes")
            .is_some_and(|g| g.as_object().is_some_and(|m| !m.is_empty()))
    {
        return Err(err("comfyui.workflow_unsupported").into());
    }
    let links = link_table(ui.get("links"));
    let by_id: HashMap<i64, &Value> = nodes
        .iter()
        .filter_map(|n| Some((n.get("id")?.as_i64()?, n)))
        .collect();
    let mut graph = Graph::new();
    for node in nodes {
        let Some(id) = node.get("id").and_then(Value::as_i64) else {
            continue;
        };
        let class = node.get("type").and_then(Value::as_str).unwrap_or("");
        let mode = node.get("mode").and_then(Value::as_i64).unwrap_or(0);
        if VIRTUAL_NODES.contains(&class) || mode == MODE_MUTED || mode == MODE_BYPASS {
            continue;
        }
        let spec = object_info
            .get(class)
            .ok_or_else(|| String::from(err("comfyui.missing_node").with("node", class)))?;
        let widgets = node.get("widgets_values");
        let mut widget_index = 0usize;
        let mut inputs = Map::new();
        for (name, entry) in ordered_inputs(spec) {
            let mut value = None;
            if is_widget(entry) {
                match widgets {
                    Some(Value::Array(values)) => {
                        value = values.get(widget_index).cloned();
                        widget_index += 1;
                        // The editor stores "control after generate" (fixed / randomize...)
                        // right after the widget it belongs to.
                        if has_control_widget(&name, entry) {
                            widget_index += 1;
                        }
                    }
                    Some(Value::Object(values)) => value = values.get(&name).cloned(),
                    _ => {}
                }
            }
            let link = node_input_link(node, &name);
            if let Some(source) = link.and_then(|l| resolve_link(l, &links, &by_id)) {
                inputs.insert(name, json!([source.0.to_string(), source.1]));
            } else if let Some(v) = value {
                inputs.insert(name, v);
            }
        }
        let title = node
            .get("title")
            .and_then(Value::as_str)
            .unwrap_or(class)
            .to_string();
        graph.insert(
            id.to_string(),
            json!({"class_type": class, "inputs": inputs, "_meta": {"title": title}}),
        );
    }
    if graph.is_empty() {
        return Err(err("comfyui.workflow_invalid").into());
    }
    Ok(graph)
}

/// `link id -> (origin node, origin slot)`; links are arrays `[id, from, slot, to, slot, type]`
/// (serialization v0.4) or objects (v1).
fn link_table(links: Option<&Value>) -> HashMap<i64, (i64, i64)> {
    let mut out = HashMap::new();
    for link in links.and_then(Value::as_array).into_iter().flatten() {
        let parsed = match link {
            Value::Array(a) => (|| {
                Some((
                    a.first()?.as_i64()?,
                    a.get(1)?.as_i64()?,
                    a.get(2)?.as_i64()?,
                ))
            })(),
            Value::Object(o) => (|| {
                Some((
                    o.get("id")?.as_i64()?,
                    o.get("origin_id")?.as_i64()?,
                    o.get("origin_slot")?.as_i64()?,
                ))
            })(),
            _ => None,
        };
        if let Some((id, from, slot)) = parsed {
            out.insert(id, (from, slot));
        }
    }
    out
}

/// Input names in the order the editor lays out their widgets: `input_order` when the
/// server sends it, otherwise the definition's own order (required, then optional).
fn ordered_inputs(spec: &Value) -> Vec<(String, &Value)> {
    let mut out = Vec::new();
    for group in ["required", "optional"] {
        let Some(defs) = spec
            .pointer(&format!("/input/{group}"))
            .and_then(Value::as_object)
        else {
            continue;
        };
        let order: Vec<&str> = spec
            .pointer(&format!("/input_order/{group}"))
            .and_then(Value::as_array)
            .map(|a| a.iter().filter_map(Value::as_str).collect())
            .unwrap_or_else(|| defs.keys().map(String::as_str).collect());
        for name in order {
            if let Some(entry) = defs.get(name) {
                out.push((name.to_string(), entry));
            }
        }
    }
    out
}

/// `[type, options]`: a list of choices or a basic type is a widget, unless forced to a socket.
fn is_widget(entry: &Value) -> bool {
    let kind = entry.get(0);
    let force_input = entry
        .get(1)
        .and_then(|o| o.get("forceInput"))
        .and_then(Value::as_bool)
        .unwrap_or(false);
    !force_input
        && match kind {
            Some(Value::Array(_)) => true,
            Some(Value::String(t)) => WIDGET_TYPES.contains(&t.as_str()),
            _ => false,
        }
}

fn has_control_widget(name: &str, entry: &Value) -> bool {
    let flagged = entry
        .get(1)
        .and_then(|o| o.get("control_after_generate"))
        .and_then(Value::as_bool);
    match flagged {
        Some(f) => f,
        None => {
            matches!(name, "seed" | "noise_seed")
                && entry.get(0).and_then(Value::as_str) == Some("INT")
        }
    }
}

fn node_input_link(node: &Value, name: &str) -> Option<i64> {
    node.get("inputs")?
        .as_array()?
        .iter()
        .find(|i| i.get("name").and_then(Value::as_str) == Some(name))?
        .get("link")?
        .as_i64()
}

/// Follows a link to the node that really produces the value: through reroutes and bypassed
/// nodes; `None` for a primitive (its value is already in the target's widget) or a muted node.
fn resolve_link(
    link: i64,
    links: &HashMap<i64, (i64, i64)>,
    by_id: &HashMap<i64, &Value>,
) -> Option<(i64, i64)> {
    let mut current = link;
    for _ in 0..MAX_LINK_HOPS {
        let &(origin, slot) = links.get(&current)?;
        let node = by_id.get(&origin)?;
        let class = node.get("type").and_then(Value::as_str).unwrap_or("");
        let mode = node.get("mode").and_then(Value::as_i64).unwrap_or(0);
        if class == "Reroute" {
            current = node.pointer("/inputs/0/link")?.as_i64()?;
        } else if mode == MODE_BYPASS {
            // A bypassed node hands on its first input of the same type as the output.
            let kind = node
                .pointer(&format!("/outputs/{slot}/type"))
                .and_then(Value::as_str)?;
            current = node
                .get("inputs")?
                .as_array()?
                .iter()
                .find(|i| i.get("type").and_then(Value::as_str) == Some(kind))?
                .get("link")?
                .as_i64()?;
        } else if class == "PrimitiveNode" || class == "Note" || mode == MODE_MUTED {
            return None;
        } else {
            return Some((origin, slot));
        }
    }
    None
}

/// Writes the run's parameters into a graph. The prompt goes to the text encoders feeding the
/// samplers' `positive` input (and the negative prompt, when given, to `negative`); seed,
/// steps and CFG to every sampler; the size to empty-latent nodes; the checkpoint to
/// `CheckpointLoaderSimple`. Fails when the graph has no text prompt to set.
pub fn apply_params(graph: &mut Graph, p: &Txt2ImgParams, seed: u64) -> Result<(), String> {
    let mut targets: Vec<(String, bool)> = Vec::new();
    for node in graph.values() {
        for (key, positive) in [("positive", true), ("negative", false)] {
            if let Some(id) = node
                .pointer(&format!("/inputs/{key}/0"))
                .and_then(Value::as_str)
            {
                targets.push((id.to_string(), positive));
            }
        }
    }
    let mut prompt_set = false;
    for (id, positive) in targets {
        if positive {
            prompt_set |= set_text(graph, &id, p.prompt.trim(), 0);
        } else if !p.negative.trim().is_empty() {
            set_text(graph, &id, p.negative.trim(), 0);
        }
    }
    if !prompt_set {
        return Err(err("comfyui.no_prompt_input").into());
    }
    for node in graph.values_mut() {
        let class = node
            .get("class_type")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string();
        let Some(inputs) = node.get_mut("inputs").and_then(Value::as_object_mut) else {
            continue;
        };
        set_number(inputs, "seed", json!(seed));
        set_number(inputs, "noise_seed", json!(seed));
        if let Some(steps) = p.steps {
            set_number(inputs, "steps", json!(steps));
        }
        if let Some(cfg) = p.cfg {
            set_number(inputs, "cfg", json!(cfg));
        }
        if class.starts_with("Empty") && class.contains("Latent") {
            if let Some(w) = p.width {
                set_number(inputs, "width", json!(w));
            }
            if let Some(h) = p.height {
                set_number(inputs, "height", json!(h));
            }
        }
        if class == "CheckpointLoaderSimple" {
            if let Some(ckpt) = p.checkpoint.as_deref().filter(|c| !c.is_empty()) {
                inputs.insert("ckpt_name".into(), json!(ckpt));
            }
        }
    }
    Ok(())
}

/// Sets a widget value; a linked input (an array) is left alone.
fn set_number(inputs: &mut Map<String, Value>, key: &str, value: Value) {
    if inputs.get(key).is_some_and(Value::is_number) {
        inputs.insert(key.into(), value);
    }
}

/// Sets the text of an encoder, following plain `conditioning` pass-through nodes.
fn set_text(graph: &mut Graph, id: &str, text: &str, depth: usize) -> bool {
    if depth > MAX_LINK_HOPS {
        return false;
    }
    let Some(inputs) = graph
        .get_mut(id)
        .and_then(|n| n.get_mut("inputs"))
        .and_then(Value::as_object_mut)
    else {
        return false;
    };
    let mut set = false;
    for key in TEXT_INPUTS {
        if inputs.get(*key).is_some_and(Value::is_string) {
            inputs.insert((*key).into(), json!(text));
            set = true;
        }
    }
    if set {
        return true;
    }
    let next = inputs
        .get("conditioning")
        .and_then(|c| c.get(0))
        .and_then(Value::as_str)
        .map(str::to_string);
    next.is_some_and(|n| set_text(graph, &n, text, depth + 1))
}

/// Picture files a finished run left, from its `/history` entry: saved outputs first;
/// previews (`temp`) only when nothing was saved.
pub fn output_images(history_entry: &Value) -> Vec<ImageRef> {
    let mut saved = Vec::new();
    let mut previews = Vec::new();
    let outputs = history_entry.get("outputs").and_then(Value::as_object);
    for node in outputs.into_iter().flat_map(|o| o.values()) {
        for image in node
            .get("images")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
        {
            let Some(r) = ImageRef::parse(image) else {
                continue;
            };
            if r.kind == "output" {
                saved.push(r);
            } else {
                previews.push(r);
            }
        }
    }
    if saved.is_empty() {
        previews
    } else {
        saved
    }
}

/// One picture on the ComfyUI side (`/view?filename=&subfolder=&type=`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImageRef {
    pub filename: String,
    pub subfolder: String,
    /// `output` or `temp`.
    pub kind: String,
}

impl ImageRef {
    fn parse(v: &Value) -> Option<Self> {
        let filename = v.get("filename")?.as_str()?.to_string();
        let kind = v.get("type").and_then(Value::as_str).unwrap_or("output");
        if filename.is_empty() || !matches!(kind, "output" | "temp") {
            return None;
        }
        Some(Self {
            filename,
            subfolder: v
                .get("subfolder")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string(),
            kind: kind.to_string(),
        })
    }
}
