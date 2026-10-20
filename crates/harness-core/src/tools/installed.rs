//! Installed tools: packages ([`super::package`]) kept in
//! `<data dir>/tools/<name>/`, registered in the [`ToolRegistry`] with source `installed`.
//!
//! The permission class comes from the manifest ([`Manifest::class`]) and is fixed at
//! registration, so the policy, the approval prompt and the tool log treat an installed tool
//! like every other tool. The code runs in [`super::js_engine`].
//!
//! `<root>/state.json` holds the one piece of state that is not in a package: the names the
//! user turned off.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use futures_util::future::BoxFuture;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::js_engine;
use super::package::{self, Manifest, Package, MANIFEST_FILE};
use super::registry::{Tool, ToolCtx, ToolRegistry};
use super::types::*;
use crate::error_key::err;

const STATE_FILE: &str = "state.json";
/// Limits of a package folder taken in by [`install`].
const MAX_FILES: usize = 200;
const MAX_TOTAL_BYTES: u64 = 5 * 1024 * 1024;
const MAX_DEPTH: usize = 6;

/// `<data dir>/tools`.
pub fn tools_root() -> PathBuf {
    crate::paths::data_root().join("tools")
}

/// One folder under the root, valid or not.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct InstalledTool {
    #[serde(default)]
    pub user_created: bool,
    /// The folder name (the tool name when the package is valid).
    pub name: String,
    pub version: String,
    #[serde(default = "package::default_category")]
    pub category: String,
    pub description: String,
    pub output: String,
    pub network: Vec<String>,
    pub fs_read: Vec<String>,
    pub fs_write: Vec<String>,
    pub class: PermissionClass,
    pub enabled: bool,
    pub has_test: bool,
    pub readme: Option<String>,
    /// Keyed error when the package is no longer valid (it is then not registered).
    pub error: Option<String>,
}

impl InstalledTool {
    fn of(pkg: &Package, enabled: bool) -> Self {
        Self::from_manifest(&pkg.manifest, pkg.readme.clone(), enabled)
    }

    /// What the lists and prompts show of a manifest (also of a generated tool's draft).
    pub fn from_manifest(m: &Manifest, readme: Option<String>, enabled: bool) -> Self {
        Self {
            user_created: false,
            name: m.name.clone(),
            version: m.version.clone(),
            category: m.category.clone(),
            description: m.description.clone(),
            output: m.output.clone(),
            network: m.permissions.network.clone(),
            fs_read: m.permissions.fs.read.clone(),
            fs_write: m.permissions.fs.write.clone(),
            class: m.class(),
            enabled,
            has_test: m.test.is_some(),
            readme,
            error: None,
        }
    }
}

#[derive(Default, Serialize, Deserialize)]
struct State {
    #[serde(default)]
    created: BTreeSet<String>,
    #[serde(default)]
    disabled: BTreeSet<String>,
}

fn load_state(root: &Path) -> State {
    std::fs::read_to_string(root.join(STATE_FILE))
        .ok()
        .and_then(|t| serde_json::from_str(&t).ok())
        .unwrap_or_default()
}

fn save_state(root: &Path, state: &State) -> Result<(), String> {
    std::fs::create_dir_all(root).map_err(io_err)?;
    let text = serde_json::to_string_pretty(state).map_err(io_err)?;
    std::fs::write(root.join(STATE_FILE), text).map_err(io_err)
}

fn io_err(e: impl std::fmt::Display) -> String {
    err("tools.package.io").with("error", e).into()
}

/// An installed tool as the registry holds it.
struct PackageTool {
    def: ToolDef,
    manifest: Manifest,
    code: String,
}

impl Tool for PackageTool {
    fn def(&self) -> &ToolDef {
        &self.def
    }

    fn call<'a>(
        &'a self,
        args: Value,
        ctx: &'a ToolCtx,
    ) -> BoxFuture<'a, Result<ToolOutput, ToolError>> {
        let ctx = ctx.clone();
        let handle = tokio::runtime::Handle::current();
        let (code, perms) = (self.code.clone(), self.manifest.permissions.clone());
        Box::pin(async move {
            tokio::task::spawn_blocking(move || {
                js_engine::run_blocking(&code, &args, &perms, &ctx, handle)
            })
            .await
            .unwrap_or_else(|e| Err(ToolError::failed(format!("the tool crashed: {e}"))))
        })
    }
}

fn tool_of(pkg: &Package) -> Result<PackageTool, String> {
    Ok(package_tool(
        &pkg.manifest,
        package::read_entry(pkg)?,
        ToolSource::Installed,
    ))
}

fn package_tool(m: &Manifest, code: String, source: ToolSource) -> PackageTool {
    PackageTool {
        def: ToolDef {
            name: m.name.clone(),
            description: m.description.clone(),
            input_schema: m.input_schema.clone(),
            class: m.class(),
            source,
            uses_workspace: m.uses_workspace(),
        },
        manifest: m.clone(),
        code,
    }
}

/// A tool made of a manifest and code that are not in a folder yet (a generated tool, kept in
/// memory until the user saves it). Same class, policy and log as an installed one.
pub(crate) fn in_memory_tool(m: &Manifest, code: &str) -> Arc<dyn Tool> {
    Arc::new(package_tool(m, code.to_string(), ToolSource::Generated))
}

/// The packages under `root` with their state; folders that fail to load are listed with the
/// error.
pub fn list(root: &Path) -> Vec<InstalledTool> {
    let state = load_state(root);
    let Ok(dir) = std::fs::read_dir(root) else {
        return Vec::new();
    };
    let mut out: Vec<InstalledTool> = dir
        .flatten()
        .filter(|e| e.file_type().is_ok_and(|t| t.is_dir()))
        .filter(|e| !e.file_name().to_string_lossy().starts_with('.'))
        .map(|e| {
            let name = e.file_name().to_string_lossy().to_string();
            let enabled = !state.disabled.contains(&name);
            let user_created = state.created.contains(&name);
            let mut info = match package::load_dir(&e.path()) {
                Ok(pkg) if pkg.manifest.name == name => InstalledTool::of(&pkg, enabled),
                Ok(pkg) => broken(
                    name,
                    enabled,
                    err("tools.package.name_mismatch")
                        .with("name", &pkg.manifest.name)
                        .into(),
                ),
                Err(e) => broken(name, enabled, e),
            };
            info.user_created = user_created;
            info
        })
        .collect();
    out.sort_by(|a, b| a.name.cmp(&b.name));
    out
}

fn broken(name: String, enabled: bool, error: String) -> InstalledTool {
    InstalledTool {
        user_created: false,
        name,
        version: String::new(),
        category: package::default_category(),
        description: String::new(),
        output: String::new(),
        network: Vec::new(),
        fs_read: Vec::new(),
        fs_write: Vec::new(),
        class: PermissionClass::Dangerous,
        enabled,
        has_test: false,
        readme: None,
        error: Some(error),
    }
}

/// Makes the registry's installed tools match the folders under `root`: every enabled, valid
/// package registered (an updated one replaced), everything else removed. A package whose name
/// is a built-in or MCP tool is not registered (it would shadow it). Returns what was left out,
/// as `(name, keyed error)`.
pub fn sync(root: &Path, registry: &ToolRegistry) -> Vec<(String, String)> {
    for def in registry.list() {
        if def.source == ToolSource::Installed {
            registry.unregister(&def.name);
        }
    }
    let mut skipped = Vec::new();
    for info in list(root) {
        if let Some(error) = info.error {
            skipped.push((info.name, error));
            continue;
        }
        if !info.enabled {
            continue;
        }
        if registry
            .get(&info.name)
            .is_some_and(|t| t.def().source != ToolSource::Installed)
        {
            skipped.push((
                info.name.clone(),
                err("tools.package.name_taken")
                    .with("name", &info.name)
                    .into(),
            ));
            continue;
        }
        let loaded = package::load_dir(&root.join(&info.name)).and_then(|p| tool_of(&p));
        match loaded {
            Ok(tool) => registry.register(Arc::new(tool)),
            Err(e) => skipped.push((info.name, e)),
        }
    }
    skipped
}

/// Validates the package in `src` without installing it (the install dialog shows what it asks
/// for first).
pub fn inspect(src: &Path) -> Result<InstalledTool, String> {
    let pkg = package::load_dir(src)?;
    package::read_entry(&pkg)?;
    Ok(InstalledTool::of(&pkg, true))
}

/// Copies `src` into `<root>/<name>/` (replacing an older version of the same tool), after the
/// manifest validated. Only regular files are copied: a link in the source is skipped, never
/// followed. The caller re-runs [`sync`].
pub fn install(root: &Path, src: &Path, registry: &ToolRegistry) -> Result<InstalledTool, String> {
    install_with_origin(root, src, registry, false)
}

pub(crate) fn install_with_origin(
    root: &Path,
    src: &Path,
    registry: &ToolRegistry,
    user_created: bool,
) -> Result<InstalledTool, String> {
    let pkg = package::load_dir(src)?;
    package::read_entry(&pkg)?;
    let name = pkg.manifest.name.clone();
    if registry
        .get(&name)
        .is_some_and(|t| t.def().source != ToolSource::Installed)
    {
        return Err(err("tools.package.name_taken").with("name", &name).into());
    }
    std::fs::create_dir_all(root).map_err(io_err)?;
    let staging = root.join(format!(".installing-{name}"));
    let _ = std::fs::remove_dir_all(&staging);
    let (mut files, mut bytes) = (0usize, 0u64);
    if let Err(e) = copy_tree(src, &staging, 0, &mut files, &mut bytes) {
        let _ = std::fs::remove_dir_all(&staging);
        return Err(e);
    }
    let target = root.join(&name);
    let backup = root.join(format!(".replaced-{name}"));
    let _ = std::fs::remove_dir_all(&backup);
    if target.exists() {
        std::fs::rename(&target, &backup).map_err(io_err)?;
    }
    if let Err(e) = std::fs::rename(&staging, &target) {
        if backup.exists() {
            let _ = std::fs::rename(&backup, &target);
        }
        let _ = std::fs::remove_dir_all(&staging);
        return Err(io_err(e));
    }
    let _ = std::fs::remove_dir_all(&backup);
    let mut state = load_state(root);
    state.disabled.remove(&name);
    if user_created {
        state.created.insert(name.clone());
    } else {
        state.created.remove(&name);
    }
    save_state(root, &state)?;
    let installed = package::load_dir(&target)?;
    let mut info = InstalledTool::of(&installed, true);
    info.user_created = user_created;
    Ok(info)
}

fn copy_tree(
    from: &Path,
    to: &Path,
    depth: usize,
    files: &mut usize,
    bytes: &mut u64,
) -> Result<(), String> {
    if depth > MAX_DEPTH {
        return Err(err("tools.package.too_large").into());
    }
    std::fs::create_dir_all(to).map_err(io_err)?;
    for entry in std::fs::read_dir(from).map_err(io_err)?.flatten() {
        let Ok(kind) = entry.file_type() else {
            continue;
        };
        let name = entry.file_name();
        if name.to_string_lossy().starts_with('.') {
            continue;
        }
        if kind.is_dir() {
            copy_tree(&entry.path(), &to.join(&name), depth + 1, files, bytes)?;
        } else if kind.is_file() {
            *files += 1;
            *bytes += entry.metadata().map_err(io_err)?.len();
            if *files > MAX_FILES || *bytes > MAX_TOTAL_BYTES {
                return Err(err("tools.package.too_large").into());
            }
            std::fs::copy(entry.path(), to.join(&name)).map_err(io_err)?;
        }
    }
    Ok(())
}

/// Deletes an installed tool's folder. The caller re-runs [`sync`].
pub fn remove(root: &Path, name: &str) -> Result<(), String> {
    if !package::valid_name(name) || !root.join(name).join(MANIFEST_FILE).exists() {
        return Err(err("tools.package.not_installed").with("name", name).into());
    }
    std::fs::remove_dir_all(root.join(name)).map_err(io_err)?;
    let mut state = load_state(root);
    state.disabled.remove(name);
    state.created.remove(name);
    save_state(root, &state)?;
    Ok(())
}

/// Turns an installed tool on or off. The caller re-runs [`sync`].
pub fn set_enabled(root: &Path, name: &str, enabled: bool) -> Result<(), String> {
    if !package::valid_name(name) || !root.join(name).join(MANIFEST_FILE).exists() {
        return Err(err("tools.package.not_installed").with("name", name).into());
    }
    let mut state = load_state(root);
    if enabled {
        state.disabled.remove(name);
    } else {
        state.disabled.insert(name.to_string());
    }
    save_state(root, &state)
}

#[cfg(test)]
mod tests;
