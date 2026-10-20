//! Offline module bundles (`.hsmod`): export an installed module, install one without a network.
//!
//! A bundle is a ZIP archive: `manifest.json` at the root (module id and version, platform, and
//! per component the install roots, every file with its SHA-256 and size, and symlinks) plus the
//! files under `payload/<path relative to the data bin dir>`. Export writes it deterministically
//! (sorted entries, fixed timestamps), so the same install always gives the same bundle hash.
//!
//! Import trusts nothing in the file until the whole-file SHA-256 matches an entry of the
//! official index (`official_bundles.json`, compiled into the binary): module id, version and
//! hash must agree. Then every file is checked against the manifest while it is staged in a
//! temporary folder next to the live ones, and only then are the live folders touched, through
//! the same journal ([`lifecycle::begin_at`] / [`lifecycle::rollback_at`]) the online install
//! uses. Shared components that are already on disk are reused and only gain an owner.
use super::lifecycle::{self, SharedRuntime};
use super::*;
use sha2::{Digest, Sha256};
use std::{
    collections::HashSet,
    io::{Read, Write},
    path::{Path, PathBuf},
};

/// Bundle layout version.
const FORMAT: u32 = 1;
const MANIFEST: &str = "manifest.json";
const PAYLOAD: &str = "payload/";
const MAX_MANIFEST_BYTES: u64 = 64 * 1024 * 1024;
/// File extension of a bundle.
pub const EXTENSION: &str = "hsmod";

/// One released bundle: the module, its version and the SHA-256 of the `.hsmod` file.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct OfficialBundle {
    pub id: String,
    pub version: String,
    pub sha256: String,
}

/// The official index compiled into the binary (empty until a bundle is released).
pub(super) fn official_index() -> Vec<OfficialBundle> {
    serde_json::from_str(include_str!("official_bundles.json")).expect("official bundle index")
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct BundleManifest {
    format: u32,
    module_id: String,
    version: String,
    platform: String,
    app_version: String,
    /// The data bin dir the bundle was made in (rewritten in venv config files on import).
    base: String,
    components: Vec<BundleComponent>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct BundleComponent {
    id: String,
    version: String,
    /// Install folders, relative to the bin dir, `/` separated.
    roots: Vec<String>,
    files: Vec<BundleFile>,
    links: Vec<BundleLink>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct BundleFile {
    path: String,
    size: u64,
    sha256: String,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct BundleLink {
    path: String,
    target: String,
}

/// What `export` wrote. `sha256` is the hash to record in `official_bundles.json`.
#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct ExportReport {
    pub id: String,
    pub version: String,
    pub path: String,
    pub bytes: u64,
    pub files: usize,
    pub sha256: String,
}
/// What `import` did.
#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct ImportReport {
    pub id: String,
    pub version: String,
    pub installed_components: Vec<String>,
    /// Shared components that were already installed: only their owner list changed.
    pub reused_components: Vec<String>,
}

/// Where a module lives: the bin dir and the module state file.
pub(super) struct Layout {
    pub base: PathBuf,
    pub store_file: PathBuf,
}
impl Layout {
    pub(super) fn global() -> Self {
        Self {
            base: paths::bin_dir(),
            store_file: paths::config_dir().join("modules.json"),
        }
    }
    fn store(&self) -> JsonStore {
        JsonStore::new(&self.store_file)
    }
    fn load(&self) -> Result<ModuleStore, String> {
        if !self.store_file.exists() {
            return Ok(ModuleStore::default());
        }
        self.store().load()
    }
    /// The root `envs::root()` is (`bin/pyenv`).
    fn pyroot(&self) -> PathBuf {
        self.base.join("pyenv")
    }
}

/// Seams of the import: production wires the real checks, tests inject failures.
pub(super) struct Hooks<'a> {
    /// The installable manifest of a module id (bundled, compatible, not planned).
    pub resolve: &'a dyn Fn(&str) -> Result<ModuleManifest, String>,
    /// Last check before the live folders change (`true`: shared folders will be written).
    pub before_commit: &'a dyn Fn(&str, bool) -> Result<(), String>,
    /// Proves the placed module runs.
    pub verify: &'a dyn Fn(&str) -> Result<(), String>,
    /// Called after each component folder is in place.
    pub after_move: &'a dyn Fn(&str) -> Result<(), String>,
}

fn corrupt() -> String {
    "modules.bundle_corrupt".into()
}
fn mismatch() -> String {
    "modules.bundle_mismatch".into()
}

fn sha256_file(path: &Path) -> Result<String, String> {
    let mut file = std::fs::File::open(path).map_err(read_failed(path))?;
    let mut hasher = Sha256::new();
    let mut buf = vec![0u8; 256 * 1024];
    loop {
        let n = file.read(&mut buf).map_err(read_failed(path))?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    Ok(crate::hex::lower(&hasher.finalize()))
}
fn read_failed(path: &Path) -> impl Fn(std::io::Error) -> String + '_ {
    move |e| {
        crate::error_key::err("fs.read_failed")
            .with("path", path.display())
            .with("error", e)
            .into()
    }
}

/// A `/`-separated relative path made only of normal names.
fn valid_rel(rel: &str) -> bool {
    !rel.is_empty()
        && rel
            .split('/')
            .all(|p| !p.is_empty() && p != "." && p != ".." && !p.contains([':', '\\', '\0']))
}
fn rel_to_path(base: &Path, rel: &str) -> PathBuf {
    rel.split('/')
        .fold(base.to_path_buf(), |p, part| p.join(part))
}
fn under(rel: &str, root: &str) -> bool {
    rel.strip_prefix(root)
        .is_some_and(|rest| rest.starts_with('/'))
}

/// Install folders a component of an installed module has (export side).
fn export_roots(layout: &Layout, component: &Component) -> Result<Vec<String>, String> {
    Ok(match component.id.as_str() {
        ENV => vec![format!("pyenv/envs/{ENV}")],
        "hermes-agent" => vec!["modules/hermes".into()],
        "managed-uv" => vec![format!("pyenv/uv/{}", component.version)],
        "managed-python" => {
            let prefix = format!("cpython-{}.", pyenv::PYTHON_SERIES);
            let mut names: Vec<String> = std::fs::read_dir(layout.pyroot().join("python"))
                .into_iter()
                .flatten()
                .flatten()
                .filter(|e| e.file_type().is_ok_and(|t| t.is_dir()))
                .filter_map(|e| e.file_name().into_string().ok())
                .filter(|n| n.starts_with(&prefix) && !n.ends_with(".module-rollback"))
                .collect();
            names.sort();
            names
                .into_iter()
                .map(|n| format!("pyenv/python/{n}"))
                .collect()
        }
        _ => return Err("modules.unknown".into()),
    })
}
/// Whether an imported bundle may put `component` into `root` (never anywhere else).
fn root_allowed(component: &BundleComponent, root: &str) -> bool {
    if !valid_rel(root) {
        return false;
    }
    match component.id.as_str() {
        ENV => root == format!("pyenv/envs/{ENV}"),
        "hermes-agent" => root == "modules/hermes",
        "managed-uv" => root == format!("pyenv/uv/{}", component.version),
        "managed-python" => {
            let parts: Vec<&str> = root.split('/').collect();
            parts.len() == 3
                && parts[..2] == ["pyenv", "python"]
                && parts[2].starts_with(&format!("cpython-{}.", pyenv::PYTHON_SERIES))
                && !parts[2].ends_with(".module-rollback")
        }
        _ => false,
    }
}

#[derive(Default)]
struct Collected {
    files: Vec<(String, PathBuf)>,
    links: Vec<BundleLink>,
}
fn walk(abs: &Path, rel: &str, out: &mut Collected) -> Result<(), String> {
    let mut entries: Vec<_> = std::fs::read_dir(abs)
        .map_err(read_failed(abs))?
        .collect::<Result<_, _>>()
        .map_err(read_failed(abs))?;
    entries.sort_by_key(|e| e.file_name());
    for entry in entries {
        let name = entry
            .file_name()
            .into_string()
            .map_err(|_| "modules.unsafe_path".to_string())?;
        if name.contains(['/', '\\', ':']) {
            return Err("modules.unsafe_path".into());
        }
        let path = entry.path();
        let child = format!("{rel}/{name}");
        let kind = std::fs::symlink_metadata(&path)
            .map_err(read_failed(&path))?
            .file_type();
        if kind.is_symlink() {
            if !cfg!(unix) {
                return Err("modules.unsafe_path".into());
            }
            let target = std::fs::read_link(&path).map_err(read_failed(&path))?;
            out.links.push(BundleLink {
                path: child,
                target: target.to_string_lossy().into_owned(),
            });
        } else if kind.is_dir() {
            walk(&path, &child, out)?;
        } else if kind.is_file() {
            out.files.push((child, path));
        }
    }
    Ok(())
}

#[cfg(unix)]
fn file_mode(path: &Path) -> u32 {
    use std::os::unix::fs::PermissionsExt;
    std::fs::metadata(path).map_or(0o644, |m| m.permissions().mode() & 0o777)
}
#[cfg(not(unix))]
fn file_mode(_: &Path) -> u32 {
    0o644
}

/// Writes the bundle of an installed module to `out`. The caller holds the module locks.
pub(super) fn export_at(
    layout: &Layout,
    manifest: &ModuleManifest,
    data: &ModuleStore,
    platform: &str,
    app: &str,
    out: &Path,
) -> Result<ExportReport, String> {
    if manifest.planned || manifest.components.is_empty() {
        return Err("modules.planned".into());
    }
    let installed = data
        .modules
        .get(&manifest.id)
        .is_some_and(|s| matches!(s.status, Status::Installed | Status::Disabled));
    if !installed {
        return Err("modules.not_installed".into());
    }
    if manifest.components.iter().any(|c| {
        data.components
            .get(&c.id)
            .is_some_and(|o| o.version.starts_with("external:"))
    }) {
        return Err("modules.export_external".into());
    }
    let out = &std::path::absolute(out).unwrap_or_else(|_| out.to_path_buf());
    let mut components = Vec::new();
    let mut sources: Vec<(String, PathBuf)> = Vec::new();
    for spec in &manifest.components {
        let roots = export_roots(layout, spec)?;
        let mut collected = Collected::default();
        if roots.is_empty() {
            return Err("modules.export_missing".into());
        }
        for root in &roots {
            let abs = rel_to_path(&layout.base, root);
            if !abs.is_dir() || out.starts_with(&abs) {
                return Err(if abs.is_dir() {
                    "modules.unsafe_path"
                } else {
                    "modules.export_missing"
                }
                .into());
            }
            walk(&abs, root, &mut collected)?;
        }
        sources.extend(collected.files.iter().cloned());
        components.push((spec, roots, collected));
    }
    if let Some(dir) = out.parent().filter(|d| !d.as_os_str().is_empty()) {
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    let mut part = out.as_os_str().to_owned();
    part.push(".part");
    let part = PathBuf::from(part);
    let written = (|| -> Result<BundleManifest, String> {
        let mut zip = zip::ZipWriter::new(std::fs::File::create(&part).map_err(|e| e.to_string())?);
        let options = |mode: u32| {
            zip::write::SimpleFileOptions::default()
                .compression_method(zip::CompressionMethod::Deflated)
                .last_modified_time(zip::DateTime::default())
                .unix_permissions(mode)
        };
        let mut described = Vec::new();
        let mut buf = vec![0u8; 256 * 1024];
        for (spec, roots, collected) in &components {
            let mut files = Vec::new();
            for (rel, abs) in &collected.files {
                let mut input = std::fs::File::open(abs).map_err(read_failed(abs))?;
                zip.start_file(format!("{PAYLOAD}{rel}"), options(file_mode(abs)))
                    .map_err(|e| e.to_string())?;
                let mut hasher = Sha256::new();
                let mut size = 0u64;
                loop {
                    let n = input.read(&mut buf).map_err(read_failed(abs))?;
                    if n == 0 {
                        break;
                    }
                    hasher.update(&buf[..n]);
                    zip.write_all(&buf[..n]).map_err(|e| e.to_string())?;
                    size += n as u64;
                }
                files.push(BundleFile {
                    path: rel.clone(),
                    size,
                    sha256: crate::hex::lower(&hasher.finalize()),
                });
            }
            described.push(BundleComponent {
                id: spec.id.clone(),
                version: spec.version.clone(),
                roots: roots.clone(),
                files,
                links: collected.links.clone(),
            });
        }
        let bundle = BundleManifest {
            format: FORMAT,
            module_id: manifest.id.clone(),
            version: manifest.version.clone(),
            platform: platform.into(),
            app_version: app.into(),
            base: layout.base.to_string_lossy().into_owned(),
            components: described,
        };
        zip.start_file(MANIFEST, options(0o644))
            .map_err(|e| e.to_string())?;
        zip.write_all(&serde_json::to_vec_pretty(&bundle).map_err(|e| e.to_string())?)
            .map_err(|e| e.to_string())?;
        zip.finish().map_err(|e| e.to_string())?;
        Ok(bundle)
    })();
    let bundle = match written {
        Ok(bundle) => bundle,
        Err(error) => {
            let _ = std::fs::remove_file(&part);
            return Err(error);
        }
    };
    let sha256 = sha256_file(&part)?;
    let bytes = std::fs::metadata(&part).map_err(|e| e.to_string())?.len();
    std::fs::rename(&part, out).map_err(|e| {
        let _ = std::fs::remove_file(&part);
        e.to_string()
    })?;
    Ok(ExportReport {
        id: bundle.module_id,
        version: bundle.version,
        path: out.display().to_string(),
        bytes,
        files: sources.len(),
        sha256,
    })
}

/// Rewrites an absolute link target made in `old` to the same place in `new`; `None` when the
/// target points outside the bin dir or climbs out of it.
fn relocate_link(old: &str, new: &Path, link: &str, target: &str) -> Option<PathBuf> {
    let norm = |s: &str| s.replace('\\', "/");
    let (t, o) = (norm(target), norm(old).trim_end_matches('/').to_string());
    if let Some(rest) = t.strip_prefix(&format!("{o}/")) {
        return valid_rel(rest).then(|| rel_to_path(new, rest));
    }
    if t.starts_with('/') || t.contains(':') {
        return None;
    }
    // Relative: resolved from the link's folder, it must stay inside the bin dir.
    let mut parts: Vec<&str> = link.split('/').collect();
    parts.pop();
    for part in t.split('/') {
        match part {
            "" | "." => {}
            ".." => {
                parts.pop()?;
            }
            p => parts.push(p),
        }
    }
    Some(PathBuf::from(target))
}

/// A moved data folder leaves absolute paths in `pyvenv.cfg`; point them at the new place.
fn relocate_venv_config(dir: &Path, old: &str, new: &str) -> Result<(), String> {
    if old == new {
        return Ok(());
    }
    for entry in std::fs::read_dir(dir).map_err(|e| e.to_string())?.flatten() {
        let path = entry.path();
        let kind = entry.file_type().map_err(|e| e.to_string())?;
        if kind.is_dir() {
            relocate_venv_config(&path, old, new)?;
        } else if kind.is_file() && entry.file_name() == "pyvenv.cfg" {
            let text = std::fs::read_to_string(&path).map_err(|e| e.to_string())?;
            let mut changed = text.clone();
            for (from, to) in [
                (old.to_string(), new.to_string()),
                (old.replace('\\', "/"), new.replace('\\', "/")),
                (old.replace('/', "\\"), new.replace('/', "\\")),
            ] {
                changed = changed.replace(&from, &to);
            }
            if changed != text {
                std::fs::write(&path, changed).map_err(|e| e.to_string())?;
            }
        }
    }
    Ok(())
}

/// Checks one payload file against its manifest entry while writing it to `dest` (`None`:
/// check only).
fn stage_file(
    archive: &mut zip::ZipArchive<std::fs::File>,
    file: &BundleFile,
    dest: Option<&Path>,
) -> Result<(), String> {
    let mut entry = archive
        .by_name(&format!("{PAYLOAD}{}", file.path))
        .map_err(|_| corrupt())?;
    if entry.size() != file.size || entry.is_dir() {
        return Err(corrupt());
    }
    #[cfg(unix)]
    let mode = entry.unix_mode();
    let mut out = match dest {
        Some(dest) => {
            if let Some(parent) = dest.parent() {
                std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
            }
            Some(std::fs::File::create(dest).map_err(|e| e.to_string())?)
        }
        None => None,
    };
    let mut hasher = Sha256::new();
    let mut total = 0u64;
    let mut buf = vec![0u8; 256 * 1024];
    let mut limited = (&mut entry).take(file.size.saturating_add(1));
    loop {
        let n = limited.read(&mut buf).map_err(|_| corrupt())?;
        if n == 0 {
            break;
        }
        total += n as u64;
        hasher.update(&buf[..n]);
        if let Some(out) = &mut out {
            out.write_all(&buf[..n]).map_err(|e| e.to_string())?;
        }
    }
    if total != file.size
        || !file
            .sha256
            .eq_ignore_ascii_case(&crate::hex::lower(&hasher.finalize()))
    {
        return Err(corrupt());
    }
    #[cfg(unix)]
    if let (Some(dest), Some(mode)) = (dest, mode) {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(dest, std::fs::Permissions::from_mode(mode & 0o755 | 0o600))
            .map_err(|e| e.to_string())?;
    }
    Ok(())
}

struct RootPlan {
    component: usize,
    rel: String,
    reuse: bool,
}

/// Installs the bundle at `bundle` with no network access. The caller holds the module locks.
pub(super) fn import_at(
    layout: &Layout,
    bundle: &Path,
    index: &[OfficialBundle],
    platform: &str,
    app: &str,
    hooks: &Hooks,
    state_lock: &Mutex<()>,
) -> Result<ImportReport, String> {
    // 1. Official only: the whole file must be a released bundle, before anything is parsed.
    let sha256 = sha256_file(bundle)?;
    let official = index
        .iter()
        .find(|e| e.sha256.eq_ignore_ascii_case(&sha256))
        .ok_or("modules.bundle_unofficial")?;
    let mut archive =
        zip::ZipArchive::new(std::fs::File::open(bundle).map_err(read_failed(bundle))?)
            .map_err(|_| corrupt())?;
    crate::zip_safety::validate_archive(&mut archive).map_err(|_| corrupt())?;
    let described: BundleManifest = {
        let entry = archive.by_name(MANIFEST).map_err(|_| corrupt())?;
        let mut text = Vec::new();
        entry
            .take(MAX_MANIFEST_BYTES + 1)
            .read_to_end(&mut text)
            .map_err(|_| corrupt())?;
        if text.len() as u64 > MAX_MANIFEST_BYTES {
            return Err(corrupt());
        }
        serde_json::from_slice(&text).map_err(|_| corrupt())?
    };
    if described.format != FORMAT {
        return Err("modules.bundle_incompatible".into());
    }
    if described.module_id != official.id || described.version != official.version {
        return Err(mismatch());
    }
    if described.platform != platform {
        return Err("modules.platform_incompatible".into());
    }
    // 2. The bundle must be what this app version installs for the module.
    let manifest = (hooks.resolve)(&described.module_id)?;
    manifest.compatibility(platform, app)?;
    if manifest.version != described.version
        || manifest.components.len() != described.components.len()
        || manifest.components.iter().any(|spec| {
            !described
                .components
                .iter()
                .any(|c| c.id == spec.id && c.version == spec.version)
        })
    {
        return Err(mismatch());
    }
    let id = manifest.id.clone();
    let shared = |component: &str| manifest.shared_component_ids.iter().any(|s| s == component);
    let own: Vec<&BundleComponent> = described
        .components
        .iter()
        .filter(|c| !shared(&c.id))
        .collect();
    if own.len() != 1 || own[0].roots.len() != 1 {
        return Err(mismatch());
    }
    // 3. Structure: every root and file path is one this component may use; no stray entries.
    let mut declared = HashSet::new();
    for component in &described.components {
        if component.roots.is_empty() || component.roots.iter().any(|r| !root_allowed(component, r))
        {
            return Err(mismatch());
        }
        let inside =
            |path: &str| valid_rel(path) && component.roots.iter().any(|root| under(path, root));
        for file in &component.files {
            if !inside(&file.path) || !declared.insert(file.path.clone()) {
                return Err(corrupt());
            }
        }
        for link in &component.links {
            if !inside(&link.path)
                || relocate_link(&described.base, &layout.base, &link.path, &link.target).is_none()
                || !cfg!(unix)
            {
                return Err(corrupt());
            }
        }
    }
    if archive.len() != declared.len() + 1 {
        return Err(corrupt());
    }
    // 4. Decide per root what is written and what is reused.
    let data = layout.load()?;
    if data.transaction.is_some() {
        return Err("modules.busy".into());
    }
    if data.modules.get(&id).is_some_and(|s| {
        matches!(s.status, Status::Installed | Status::Disabled)
            && s.installed_version.as_deref() == Some(manifest.version.as_str())
    }) {
        return Err("modules.already_installed".into());
    }
    let mut plan = Vec::new();
    for (i, component) in described.components.iter().enumerate() {
        if data.components.get(&component.id).is_some_and(|old| {
            old.owners.iter().any(|o| *o != id) && old.version != component.version
        }) {
            return Err("modules.shared_conflict".into());
        }
        for root in &component.roots {
            plan.push(RootPlan {
                component: i,
                rel: root.clone(),
                reuse: shared(&component.id) && rel_to_path(&layout.base, root).is_dir(),
            });
        }
    }
    let writing = |component: &BundleComponent, file: &str| {
        plan.iter().any(|p| {
            !p.reuse && described.components[p.component].id == component.id && under(file, &p.rel)
        })
    };
    let needed: u64 = described
        .components
        .iter()
        .flat_map(|c| c.files.iter().map(move |f| (c, f)))
        .filter(|(c, f)| writing(c, &f.path))
        .map(|(_, f)| f.size)
        .sum();
    std::fs::create_dir_all(&layout.base).map_err(|e| e.to_string())?;
    let (free, total) = crate::storage::StorageManager::get_directory_disk_space(&layout.base);
    if total != 0 && free < needed.saturating_add(128 * 1024 * 1024) {
        return Err("modules.disk_space".into());
    }
    // 5. Stage everything beside the live folders; every checksum is verified here.
    let stage = tempfile::Builder::new()
        .prefix(".hsmod-import-")
        .tempdir_in(&layout.base)
        .map_err(|e| e.to_string())?;
    for component in &described.components {
        for file in &component.files {
            let dest =
                writing(component, &file.path).then(|| rel_to_path(stage.path(), &file.path));
            stage_file(&mut archive, file, dest.as_deref())?;
        }
    }
    #[cfg(unix)]
    for component in &described.components {
        for link in &component.links {
            if !writing(component, &link.path) {
                continue;
            }
            let target = relocate_link(&described.base, &layout.base, &link.path, &link.target)
                .ok_or_else(corrupt)?;
            let at = rel_to_path(stage.path(), &link.path);
            std::fs::create_dir_all(at.parent().unwrap()).map_err(|e| e.to_string())?;
            std::os::unix::fs::symlink(target, at).map_err(|e| e.to_string())?;
        }
    }
    let new_base = layout.base.to_string_lossy().into_owned();
    for p in plan.iter().filter(|p| !p.reuse) {
        let staged = rel_to_path(stage.path(), &p.rel);
        std::fs::create_dir_all(&staged).map_err(|e| e.to_string())?;
        relocate_venv_config(&staged, &described.base, &new_base)?;
    }
    // 6. Commit through the install journal; any failure from here restores everything.
    let own_root = rel_to_path(&layout.base, &own[0].roots[0]);
    let backup = own_root.with_extension("rollback");
    let pyroot = layout.pyroot();
    let shared_runtimes: Vec<SharedRuntime> = plan
        .iter()
        .filter(|p| !p.reuse && shared(&described.components[p.component].id))
        .map(|p| SharedRuntime {
            relative: PathBuf::from(p.rel.strip_prefix("pyenv/").unwrap_or(&p.rel)),
            existed: false,
        })
        .collect();
    (hooks.before_commit)(&id, !shared_runtimes.is_empty())?;
    let store = layout.store();
    let mut data;
    let previous;
    {
        let _lock = state_lock.lock().unwrap();
        data = layout.load()?;
        previous = data.clone();
        if let Err(error) = lifecycle::begin_at(
            &mut data,
            &id,
            &own_root,
            &backup,
            &store,
            &pyroot,
            shared_runtimes.clone(),
        ) {
            lifecycle::rollback_at(&mut data, &own_root, &backup, &store, &pyroot)
                .map_err(|_| "modules.rollback_failed".to_string())?;
            return Err(error);
        }
    }
    // The slow proof of a working module runs without the state lock, like the online install.
    let placed = (|| -> Result<(), String> {
        for p in plan.iter().filter(|p| !p.reuse) {
            let target = rel_to_path(&layout.base, &p.rel);
            if let Some(parent) = target.parent() {
                std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
            }
            std::fs::rename(rel_to_path(stage.path(), &p.rel), &target)
                .map_err(|e| e.to_string())?;
            (hooks.after_move)(&described.components[p.component].id)?;
        }
        (hooks.verify)(&id)
    })();
    let _lock = state_lock.lock().unwrap();
    if let Err(error) = placed {
        lifecycle::rollback_at(&mut data, &own_root, &backup, &store, &pyroot)
            .map_err(|_| "modules.rollback_failed".to_string())?;
        debug_assert_eq!(data, previous);
        return Err(error);
    }
    data.claim(&manifest);
    if previous
        .modules
        .get(&id)
        .is_some_and(|s| s.status == Status::Disabled)
    {
        data.modules.get_mut(&id).unwrap().status = Status::Disabled;
    }
    data.transaction = None;
    if let Err(error) = store.save(&data) {
        let mut data = layout.load()?;
        lifecycle::rollback_at(&mut data, &own_root, &backup, &store, &pyroot)
            .map_err(|_| "modules.rollback_failed".to_string())?;
        return Err(error);
    }
    lifecycle::erase(&backup)?;
    for entry in &shared_runtimes {
        lifecycle::erase(&lifecycle::shared_backup(&pyroot.join(&entry.relative)))?;
    }
    let (reused, installed): (Vec<_>, Vec<_>) = described
        .components
        .iter()
        .map(|c| {
            (
                c.id.clone(),
                plan.iter()
                    .filter(|p| described.components[p.component].id == c.id)
                    .all(|p| p.reuse),
            )
        })
        .partition(|(_, reused)| *reused);
    Ok(ImportReport {
        id,
        version: described.version,
        installed_components: installed.into_iter().map(|(c, _)| c).collect(),
        reused_components: reused.into_iter().map(|(c, _)| c).collect(),
    })
}

// --- public entry points (global data directory) ---------------------------------------------

fn host_platform() -> String {
    format!("{}-{}", std::env::consts::OS, std::env::consts::ARCH)
}

/// Writes the installed module `id` to a `.hsmod` bundle at `out`. Needs no network.
pub fn export(id: &str, out: &Path) -> Result<ExportReport, String> {
    let manifest = backend(id)?;
    let _operation = OPERATION
        .try_lock()
        .map_err(|_| "modules.busy".to_string())?;
    let _lock = STATE_LOCK.lock().unwrap();
    let data = load()?;
    if data.transaction.is_some() {
        return Err("modules.busy".into());
    }
    export_at(
        &Layout::global(),
        &manifest,
        &data,
        &host_platform(),
        env!("CARGO_PKG_VERSION"),
        out,
    )
}

/// Installs a module from an official `.hsmod` bundle. Needs no network.
pub async fn import(path: &Path) -> Result<ImportReport, String> {
    let path = path.to_path_buf();
    let handle = tokio::runtime::Handle::current();
    tokio::task::spawn_blocking(move || import_blocking(&path, &official_index(), &handle))
        .await
        .map_err(|e| e.to_string())?
}

fn import_blocking(
    path: &Path,
    index: &[OfficialBundle],
    handle: &tokio::runtime::Handle,
) -> Result<ImportReport, String> {
    let _refresh_disk = lifecycle::RefreshDisk;
    let _operation = OPERATION
        .try_lock()
        .map_err(|_| "modules.busy".to_string())?;
    let _build = envs::BUILD_LOCK
        .try_lock()
        .map_err(|_| "modules.busy".to_string())?;
    {
        let _lock = STATE_LOCK.lock().unwrap();
        let mut data = load()?;
        lifecycle::rollback(&mut data)?;
    }
    let resolve = |id: &str| installable(id);
    let before_commit = |id: &str, shared: bool| {
        lifecycle::unused(id)?;
        if shared {
            lifecycle::ensure_path_unused(&envs::root())?;
        }
        Ok(())
    };
    let verify = |id: &str| handle.block_on(lifecycle::verify(id));
    let after_move = |_: &str| Ok(());
    import_at(
        &Layout::global(),
        path,
        index,
        &host_platform(),
        env!("CARGO_PKG_VERSION"),
        &Hooks {
            resolve: &resolve,
            before_commit: &before_commit,
            verify: &verify,
            after_move: &after_move,
        },
        &STATE_LOCK,
    )
}

#[cfg(test)]
mod tests;
