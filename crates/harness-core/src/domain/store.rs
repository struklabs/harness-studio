//! Persistence of the domain files and the compatibility layer for the pre-v2 files.
//!
//! Files (all in the config directory, all versioned through [`crate::store`]):
//! * `model_definitions.json`: `{schema_version, definitions: [...], model_defaults: {...}}`.
//!   `model_defaults` maps a model file (LocalModel id) to the definition the Dashboard
//!   uses for it, plus the Dashboard's per-model serve settings and API key reference
//!   (what `presets.json` used to hold).
//! * `server_profiles.json`: `{schema_version, profiles: [...]}`. The profile with id
//!   `"default"` is the Dashboard server's last session (what `last_session.json` held).
//! * `inference_presets.json`: `{schema_version, presets: [...]}` (user presets only).
//!
//! **v2 layout migration:** when `presets.json`, `profiles.json` or `last_session.json`
//! exist they are imported (presets first, so profiles can reuse the per-model definition)
//! and moved to `<name>.pre-v2`. A legacy file that shows up again later (e.g. written by an
//! older build after a downgrade) is imported over the existing entries the same way.
//! Keychain entries keep their accounts (`preset:<hash>`, `profile:<id>`, `session`).

use super::launch::{
    config_layer, diff_layer, fill_definition, model_part, normalize_definition, resolve_launch,
    serve_part, snapshot_overrides, LaunchDefaults,
};
use super::presets::{builtin_presets, is_builtin_id, InferencePreset};
use super::{
    new_id, now_secs, sanitize_alias, unique_alias, validate_alias, ModelDefinition, ModelRef,
    RouterSpec, ServerProfile,
};
use crate::error_key::err;
use crate::models::ModelPreset;
use crate::params::{ParamValue, ParamValues};
use crate::process::LaunchConfig;
use crate::servers::{ServerProfile as ProfileView, DEFAULT_SERVER_ID};
use crate::store::{Envelope, JsonStore, Schema, Versioned};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

pub const DEFINITIONS_SCHEMA: Schema = Schema {
    file: "model_definitions.json",
    current: 3,
    envelope: Envelope::Merged,
    migrations: &[definitions_v1_to_v2, definitions_v2_to_v3],
};
pub const PROFILES_SCHEMA: Schema = Schema {
    file: "server_profiles.json",
    current: 2,
    envelope: Envelope::Merged,
    migrations: &[profiles_v1_to_v2],
};
pub const PRESETS_SCHEMA: Schema = Schema {
    file: "inference_presets.json",
    current: 1,
    envelope: Envelope::Merged,
    migrations: &[],
};

pub const LEGACY_PRESETS: &str = "presets.json";
pub const LEGACY_PROFILES: &str = "profiles.json";
pub const LEGACY_SESSION: &str = "last_session.json";

/// The Dashboard's state for one model file.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ModelDefault {
    pub definition_id: String,
    /// Keychain reference of the Dashboard's API key for this model (`preset:<hash>`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub api_key_ref: Option<String>,
    /// Dashboard serve settings for this model (tools, MCP...).
    #[serde(default, skip_serializing_if = "ParamValues::is_empty")]
    pub serve_overrides: ParamValues,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct DefinitionsFile {
    #[serde(default)]
    pub definitions: Vec<ModelDefinition>,
    #[serde(default)]
    pub model_defaults: BTreeMap<String, ModelDefault>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct ProfilesFile {
    #[serde(default)]
    pub profiles: Vec<ServerProfile>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct PresetsFile {
    #[serde(default)]
    pub presets: Vec<InferencePreset>,
}

/// A profile using a definition.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ProfileRef {
    pub id: String,
    pub name: String,
}

/// Everything that references a definition.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct DefinitionRefs {
    /// User profiles serving it (they block deletion).
    pub profiles: Vec<ProfileRef>,
    /// Model files whose Dashboard definition it is.
    pub model_default_for: Vec<String>,
    /// The Dashboard's last session runs it.
    pub session: bool,
}

/// Serializes every domain read-modify-write (the files reference each other).
static DOMAIN_LOCK: Mutex<()> = Mutex::new(());

fn domain_lock() -> std::sync::MutexGuard<'static, ()> {
    DOMAIN_LOCK.lock().unwrap_or_else(|e| e.into_inner())
}

/// What the v2 layout migration did (for logs and tests).
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct MigrationReport {
    pub presets: usize,
    pub profiles: usize,
    pub session: bool,
    /// Legacy files moved aside (`<name>.pre-v2`).
    pub backups: Vec<PathBuf>,
    pub errors: Vec<String>,
}

/// In-memory copy of the two linked files.
struct State {
    defs: DefinitionsFile,
    profiles: ProfilesFile,
    defs_read_only: bool,
    profiles_read_only: bool,
    defs_dirty: bool,
    profiles_dirty: bool,
}

/// Domain files of one config directory.
#[derive(Debug, Clone)]
pub struct Domain {
    dir: PathBuf,
}

impl Domain {
    pub fn at(dir: impl Into<PathBuf>) -> Self {
        Self { dir: dir.into() }
    }

    /// The app's config directory.
    pub fn global() -> Self {
        Self::at(crate::paths::config_dir())
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }

    fn defs_store(&self) -> JsonStore {
        JsonStore::sensitive(self.dir.join(DEFINITIONS_SCHEMA.file))
    }

    fn profiles_store(&self) -> JsonStore {
        JsonStore::sensitive(self.dir.join(PROFILES_SCHEMA.file))
    }

    fn presets_store(&self) -> JsonStore {
        JsonStore::new(self.dir.join(PRESETS_SCHEMA.file))
    }

    fn legacy(&self, name: &str) -> PathBuf {
        self.dir.join(name)
    }

    // --- state ---

    fn load_state(&self) -> Result<State, String> {
        let defs: Versioned<DefinitionsFile> = self
            .defs_store()
            .load_versioned_or_default(&DEFINITIONS_SCHEMA)?;
        let profiles: Versioned<ProfilesFile> = self
            .profiles_store()
            .load_versioned_or_default(&PROFILES_SCHEMA)?;
        Ok(State {
            defs: defs.data,
            profiles: profiles.data,
            defs_read_only: defs.read_only,
            profiles_read_only: profiles.read_only,
            defs_dirty: false,
            profiles_dirty: false,
        })
    }

    /// Writes what changed: definitions first, so a profile never points at a definition that
    /// isn't on disk yet.
    fn commit(&self, state: &mut State) -> Result<(), String> {
        if state.defs_dirty {
            self.defs_store()
                .save_versioned(&DEFINITIONS_SCHEMA, &state.defs)?;
            state.defs_dirty = false;
            crate::events::publish(crate::events::CoreEvent::DefinitionsChanged);
        }
        if state.profiles_dirty {
            self.profiles_store()
                .save_versioned(&PROFILES_SCHEMA, &state.profiles)?;
            state.profiles_dirty = false;
        }
        Ok(())
    }

    /// Loads the state (importing legacy files first). Caller holds the domain lock.
    fn state(&self) -> Result<State, String> {
        let mut state = self.load_state()?;
        if self.has_legacy_files() && !state.defs_read_only && !state.profiles_read_only {
            let report = self.migrate_legacy_into(&mut state);
            for e in &report.errors {
                eprintln!("[Harness Studio] v2 taşıma: {}", e);
            }
        }
        Ok(state)
    }

    fn has_legacy_files(&self) -> bool {
        [LEGACY_PRESETS, LEGACY_PROFILES, LEGACY_SESSION]
            .iter()
            .any(|n| self.legacy(n).is_file())
    }

    /// Imports the pre-v2 files now (normally done lazily by the first domain access).
    pub fn migrate_legacy(&self) -> Result<MigrationReport, String> {
        let _g = domain_lock();
        let mut state = self.load_state()?;
        if state.defs_read_only || state.profiles_read_only {
            return Ok(MigrationReport::default());
        }
        Ok(self.migrate_legacy_into(&mut state))
    }

    fn migrate_legacy_into(&self, state: &mut State) -> MigrationReport {
        let mut report = MigrationReport::default();
        let now = now_secs();

        // 1. presets.json: one definition per model file (the Dashboard's settings).
        let presets_path = self.legacy(LEGACY_PRESETS);
        let mut presets_scrub = None;
        if presets_path.is_file() {
            match read_json(&presets_path) {
                Ok(serde_json::Value::Object(map)) => {
                    let mut all_keys_adopted = true;
                    for (model_id, value) in map {
                        let mut preset: ModelPreset = match serde_json::from_value(value) {
                            Ok(p) => p,
                            Err(e) => {
                                report.errors.push(
                                    err("domain.legacy_entry_failed")
                                        .with("file", LEGACY_PRESETS)
                                        .with("id", &model_id)
                                        .with("error", e)
                                        .into(),
                                );
                                continue;
                            }
                        };
                        if preset
                            .api_key
                            .as_deref()
                            .is_some_and(|k| !k.trim().is_empty())
                        {
                            match crate::secrets::adopt_api_key(
                                &crate::secrets::preset_ref(&model_id),
                                preset.api_key.as_deref(),
                                None,
                            ) {
                                Ok(r) => preset.api_key_ref = r,
                                Err(e) => {
                                    all_keys_adopted = false;
                                    report.errors.push(e);
                                }
                            }
                        }
                        preset.api_key = None;
                        match state.upsert_model_default(&model_id, &preset, now) {
                            Ok(()) => report.presets += 1,
                            Err(e) => report.errors.push(
                                err("domain.legacy_entry_failed")
                                    .with("file", LEGACY_PRESETS)
                                    .with("id", &model_id)
                                    .with("error", e)
                                    .into(),
                            ),
                        }
                    }
                    presets_scrub = Some(all_keys_adopted);
                }
                Ok(_) => report.errors.push(
                    err("domain.legacy_not_object")
                        .with("file", LEGACY_PRESETS)
                        .into(),
                ),
                Err(e) => report.errors.push(e),
            }
        }

        // 2. profiles.json: definition (reused when identical) + server profile.
        let profiles_path = self.legacy(LEGACY_PROFILES);
        let mut profiles_scrub = None;
        if profiles_path.is_file() {
            match read_json(&profiles_path).and_then(|v| {
                serde_json::from_value::<Vec<ProfileView>>(v).map_err(|e| {
                    String::from(
                        err("json.parse_failed")
                            .with("path", profiles_path.display())
                            .with("error", e),
                    )
                })
            }) {
                Ok(list) => {
                    let mut all_keys_adopted = true;
                    for mut view in list {
                        if crate::servers::validate_id(&view.id).is_err()
                            || view.id == DEFAULT_SERVER_ID
                        {
                            report.errors.push(
                                err("domain.legacy_invalid_profile_id")
                                    .with("file", LEGACY_PROFILES)
                                    .with("id", &view.id)
                                    .into(),
                            );
                            continue;
                        }
                        if view
                            .config
                            .api_key
                            .as_deref()
                            .is_some_and(|k| !k.trim().is_empty())
                        {
                            match crate::secrets::adopt_api_key(
                                &crate::secrets::profile_ref(&view.id),
                                view.config.api_key.as_deref(),
                                view.config.api_key_ref.as_deref(),
                            ) {
                                Ok(r) => view.config.api_key_ref = r,
                                Err(e) => {
                                    all_keys_adopted = false;
                                    report.errors.push(e);
                                }
                            }
                        }
                        view.config.api_key = None;
                        let stamp = (view.updated_at > 0).then_some(view.updated_at);
                        match state.upsert_profile_view(&view, stamp.unwrap_or(now)) {
                            Ok(()) => report.profiles += 1,
                            Err(e) => report.errors.push(
                                err("domain.legacy_entry_failed")
                                    .with("file", LEGACY_PROFILES)
                                    .with("id", &view.id)
                                    .with("error", e)
                                    .into(),
                            ),
                        }
                    }
                    profiles_scrub = Some(all_keys_adopted);
                }
                Err(e) => report.errors.push(e),
            }
        }

        // 3. last_session.json: the Dashboard server ("default" profile).
        let session_path = self.legacy(LEGACY_SESSION);
        let mut session_scrub = None;
        if session_path.is_file() {
            match read_json(&session_path).and_then(|v| {
                serde_json::from_value::<LaunchConfig>(v).map_err(|e| {
                    String::from(
                        err("json.parse_failed")
                            .with("path", session_path.display())
                            .with("error", e),
                    )
                })
            }) {
                Ok(mut config) => {
                    let mut adopted = true;
                    if config
                        .api_key
                        .as_deref()
                        .is_some_and(|k| !k.trim().is_empty())
                    {
                        match crate::secrets::adopt_api_key(
                            crate::secrets::SESSION_REF,
                            config.api_key.as_deref(),
                            None,
                        ) {
                            Ok(r) => config.api_key_ref = r,
                            Err(e) => {
                                adopted = false;
                                report.errors.push(e);
                            }
                        }
                    }
                    config.api_key = None;
                    match state.set_session(&config, now) {
                        Ok(()) => report.session = true,
                        Err(e) => report.errors.push(
                            err("domain.legacy_file_failed")
                                .with("file", LEGACY_SESSION)
                                .with("error", e)
                                .into(),
                        ),
                    }
                    session_scrub = Some(adopted);
                }
                Err(e) => report.errors.push(e),
            }
        }

        if let Err(e) = self.commit(state) {
            report.errors.push(e);
            return report; // legacy files stay; the next access retries
        }
        for (path, scrub) in [
            (presets_path, presets_scrub),
            (profiles_path, profiles_scrub),
            (session_path, session_scrub),
        ] {
            let Some(scrub) = scrub else { continue };
            match retire_legacy(&path, scrub) {
                Ok(b) => report.backups.push(b),
                Err(e) => report.errors.push(e),
            }
        }
        report
    }

    // --- model definitions ---

    pub fn definitions(&self) -> Result<Vec<ModelDefinition>, String> {
        let _g = domain_lock();
        Ok(self.state()?.defs.definitions)
    }

    pub fn definitions_file(&self) -> Result<DefinitionsFile, String> {
        let _g = domain_lock();
        Ok(self.state()?.defs)
    }

    pub fn definition(&self, id: &str) -> Result<ModelDefinition, String> {
        self.definitions()?
            .into_iter()
            .find(|d| d.id == id)
            .ok_or_else(|| String::from(err("definitions.not_found").with("id", id)))
    }

    /// "Load and save to the definition": merges load settings into a definition's `load`
    /// (see [`super::launch::merge_load_overrides`]) and returns the saved definition.
    pub fn save_definition_load(
        &self,
        id: &str,
        overrides: &ParamValues,
    ) -> Result<ModelDefinition, String> {
        let mut def = self.definition(id)?;
        super::launch::merge_load_overrides(&mut def, overrides);
        self.save_definition(def)
    }

    /// Creates (empty `id`) or replaces a definition. The alias must be valid and unique
    /// (case-insensitive).
    pub fn save_definition(&self, mut def: ModelDefinition) -> Result<ModelDefinition, String> {
        let _g = domain_lock();
        let mut state = self.state()?;
        def.alias = def.alias.trim().to_string();
        validate_alias(&def.alias)?;
        if let Some(clash) = state
            .defs
            .definitions
            .iter()
            .find(|d| d.id != def.id && d.alias.eq_ignore_ascii_case(&def.alias))
        {
            return Err(err("definitions.alias_exists")
                .with("alias", &def.alias)
                .with("other", &clash.alias)
                .into());
        }
        if let ModelRef::Local { path } = &def.base {
            if path.as_os_str().is_empty() {
                return Err(err("definitions.model_file_required").into());
            }
        }
        if let Some(p) = def.default_preset_id.as_deref() {
            if !is_builtin_id(p) && !self.user_presets_locked()?.iter().any(|x| x.id == p) {
                return Err(String::from(err("presets.not_found").with("id", p)));
            }
        }
        normalize_definition(&mut def);
        def.description = def.description.trim().chars().take(500).collect();
        // Edited by the user: theirs from now on (never cleaned up automatically).
        def.auto_created = false;
        let now = now_secs();
        def.updated_at = now;
        match state.defs.definitions.iter_mut().find(|d| d.id == def.id) {
            Some(existing) if !def.id.is_empty() => {
                def.created_at = existing.created_at;
                *existing = def.clone();
            }
            _ => {
                if def.id.is_empty() {
                    def.id = new_id("md");
                }
                def.created_at = now;
                state.defs.definitions.push(def.clone());
            }
        }
        state.defs_dirty = true;
        self.commit(&mut state)?;
        Ok(def)
    }

    /// Copy with a new id and a free alias (`<alias>-copy`, `-copy-2`...).
    pub fn duplicate_definition(
        &self,
        id: &str,
        alias: Option<String>,
    ) -> Result<ModelDefinition, String> {
        let mut copy = self.definition(id)?;
        let taken: Vec<String> = self.definitions()?.into_iter().map(|d| d.alias).collect();
        copy.alias = match alias {
            Some(a) => a,
            None => unique_alias(
                &format!("{}-copy", copy.alias),
                taken.iter().map(|s| s.as_str()),
            ),
        };
        copy.id = String::new();
        self.save_definition(copy)
    }

    /// Refused while a server profile serves it. A Dashboard default pointing at it is
    /// dropped (the Dashboard then starts from defaults for that model).
    pub fn delete_definition(&self, id: &str) -> Result<(), String> {
        let _g = domain_lock();
        let mut state = self.state()?;
        if let Some(p) = state
            .profiles
            .profiles
            .iter()
            .find(|p| p.id != DEFAULT_SERVER_ID && p.definition_id.as_deref() == Some(id))
        {
            return Err(err("definitions.used_by_profile")
                .with("name", &p.name)
                .into());
        }
        let before = state.defs.definitions.len();
        state.defs.definitions.retain(|d| d.id != id);
        if state.defs.definitions.len() == before {
            return Err(String::from(err("definitions.not_found").with("id", id)));
        }
        state
            .defs
            .model_defaults
            .retain(|_, d| d.definition_id != id);
        state.defs_dirty = true;
        if let Some(session) = state
            .profiles
            .profiles
            .iter_mut()
            .find(|p| p.id == DEFAULT_SERVER_ID && p.definition_id.as_deref() == Some(id))
        {
            session.definition_id = None;
            state.profiles_dirty = true;
        }
        self.commit(&mut state)
    }

    // --- server profiles ---

    /// Every profile including the Dashboard's `"default"`.
    pub fn profiles(&self) -> Result<Vec<ServerProfile>, String> {
        let _g = domain_lock();
        Ok(self.state()?.profiles.profiles)
    }

    pub fn profile(&self, id: &str) -> Result<ServerProfile, String> {
        self.profiles()?
            .into_iter()
            .find(|p| p.id == id)
            .ok_or_else(|| String::from(err("profiles.not_found").with("id", id)))
    }

    /// Effective launch config of a profile (or of a definition served with defaults).
    pub fn resolve_profile(
        &self,
        defaults: &LaunchDefaults,
        profile_id: &str,
        transient: &ParamValues,
    ) -> Result<LaunchConfig, String> {
        let _g = domain_lock();
        let state = self.state()?;
        state.resolve_profile(defaults, profile_id, transient)
    }

    pub fn resolve_definition(
        &self,
        defaults: &LaunchDefaults,
        definition_id: &str,
        transient: &ParamValues,
    ) -> Result<LaunchConfig, String> {
        let _g = domain_lock();
        let state = self.state()?;
        let def = state
            .def(definition_id)
            .ok_or_else(|| String::from(err("definitions.not_found").with("id", definition_id)))?;
        resolve_launch(defaults, Some(def), None, transient)
    }

    // --- compatibility: the pre-v2 command shapes ---

    /// `list_server_profiles`: user profiles in the old `{config: LaunchConfig}` shape.
    pub fn profile_views(&self, defaults: &LaunchDefaults) -> Result<Vec<ProfileView>, String> {
        let _g = domain_lock();
        let state = self.state()?;
        Ok(state
            .profiles
            .profiles
            .iter()
            .filter(|p| p.id != DEFAULT_SERVER_ID)
            .filter_map(|p| match state.view(defaults, p) {
                Ok(v) => Some(v),
                Err(e) => {
                    eprintln!("[Harness Studio] Profil {} çözümlenemedi: {}", p.id, e);
                    None
                }
            })
            .collect())
    }

    /// `save_server_profile` (the API key already moved to the keychain by the caller).
    pub fn save_profile_view(&self, view: &ProfileView) -> Result<(), String> {
        let _g = domain_lock();
        let mut state = self.state()?;
        state.upsert_profile_view(view, now_secs())?;
        self.commit(&mut state)
    }

    /// Replaces the values of a (non-default) profile's `serve_overrides` for the ids of
    /// `layer` it already sets; ids it does not set keep coming from the definition. No
    /// profile with this id: nothing to do.
    pub fn update_profile_overrides(&self, id: &str, layer: &ParamValues) -> Result<(), String> {
        let _g = domain_lock();
        let mut state = self.state()?;
        let Some(profile) = state
            .profiles
            .profiles
            .iter_mut()
            .find(|p| p.id == id && p.id != DEFAULT_SERVER_ID)
        else {
            return Ok(());
        };
        let mut changed = false;
        for (k, v) in layer {
            if let Some(slot) = profile.serve_overrides.get_mut(k) {
                *slot = v.clone();
                changed = true;
            }
        }
        if !changed {
            return Ok(());
        }
        profile.updated_at = now_secs();
        state.profiles_dirty = true;
        self.commit(&mut state)
    }

    /// Deletes a profile. A definition Harness Studio created for it (`auto_created`) goes
    /// too when nothing else references it.
    pub fn delete_profile(&self, id: &str) -> Result<(), String> {
        let _g = domain_lock();
        let mut state = self.state()?;
        let owned = state
            .profiles
            .profiles
            .iter()
            .find(|p| p.id == id)
            .and_then(|p| p.definition_id.clone());
        let before = state.profiles.profiles.len();
        state.profiles.profiles.retain(|p| p.id != id);
        state.profiles_dirty = state.profiles.profiles.len() != before;
        if let Some(def_id) = owned {
            state.drop_orphan_auto(&def_id);
        }
        self.commit(&mut state)
    }

    /// Who uses a definition (shown before deleting it).
    pub fn definition_refs(&self, id: &str) -> Result<DefinitionRefs, String> {
        let _g = domain_lock();
        let state = self.state()?;
        let serves = |p: &ServerProfile| p.definition_id.as_deref() == Some(id);
        Ok(DefinitionRefs {
            profiles: state
                .profiles
                .profiles
                .iter()
                .filter(|p| p.id != DEFAULT_SERVER_ID && serves(p))
                .map(|p| ProfileRef {
                    id: p.id.clone(),
                    name: p.name.clone(),
                })
                .collect(),
            model_default_for: state
                .defs
                .model_defaults
                .iter()
                .filter(|(_, d)| d.definition_id == id)
                .map(|(k, _)| k.clone())
                .collect(),
            session: state
                .profiles
                .profiles
                .iter()
                .any(|p| p.id == DEFAULT_SERVER_ID && serves(p)),
        })
    }

    /// The Dashboard form of a definition: its load settings plus the Dashboard's serve
    /// settings and API key reference for the definition's file (`model_defaults`).
    pub fn definition_form(
        &self,
        defaults: &LaunchDefaults,
        definition_id: &str,
    ) -> Result<LaunchConfig, String> {
        self.definition_form_with(defaults, definition_id, &Default::default())
    }

    /// [`Self::definition_form`] with one-time load `overrides` on top (the definition is not
    /// changed): the Playground's "Load Settings".
    pub fn definition_form_with(
        &self,
        defaults: &LaunchDefaults,
        definition_id: &str,
        overrides: &crate::params::ParamValues,
    ) -> Result<LaunchConfig, String> {
        let _g = domain_lock();
        let state = self.state()?;
        let def = state
            .def(definition_id)
            .ok_or_else(|| String::from(err("definitions.not_found").with("id", definition_id)))?;
        let entry = def.base.local_path().and_then(|p| {
            state
                .defs
                .model_defaults
                .get(&p.to_string_lossy().to_string())
        });
        let mut layer = entry.map(|e| e.serve_overrides.clone()).unwrap_or_default();
        layer.extend(overrides.iter().map(|(k, v)| (k.clone(), v.clone())));
        let def = super::launch::definition_for_load(def, overrides);
        let mut config = resolve_launch(defaults, Some(&def), None, &layer)?;
        config.api_key_ref = entry.and_then(|e| e.api_key_ref.clone());
        Ok(config)
    }

    /// The Dashboard's definition for a model file: its `model_defaults` entry, else the first
    /// definition based on the file.
    pub fn dashboard_definition(&self, model_id: &str) -> Result<Option<String>, String> {
        let _g = domain_lock();
        let state = self.state()?;
        if let Some(e) = state
            .defs
            .model_defaults
            .get(model_id)
            .filter(|e| state.def(&e.definition_id).is_some())
        {
            return Ok(Some(e.definition_id.clone()));
        }
        let path = Path::new(model_id);
        Ok(state
            .defs
            .definitions
            .iter()
            .find(|d| d.base.local_path() == Some(path))
            .map(|d| d.id.clone()))
    }

    /// "Save as default" / "Save as new definition" from a launch form: the model-level part
    /// of `config` goes into the definition (`target = None` creates one named `new_alias`),
    /// the serve part and `config.api_key_ref` (already in the keychain) become the
    /// Dashboard's settings for the file, and the definition becomes the file's Dashboard
    /// definition.
    pub fn save_form(
        &self,
        target: Option<&str>,
        new_alias: Option<&str>,
        config: &LaunchConfig,
    ) -> Result<ModelDefinition, String> {
        if config.router_mode {
            return Err(err("definitions.router_mode").into());
        }
        if config.model_path.as_os_str().is_empty() {
            return Err(err("definitions.model_file_required").into());
        }
        let _g = domain_lock();
        let mut state = self.state()?;
        let now = now_secs();
        let def_id = match target {
            Some(id) => {
                let d = state
                    .defs
                    .definitions
                    .iter_mut()
                    .find(|d| d.id == id)
                    .ok_or_else(|| String::from(err("definitions.not_found").with("id", id)))?;
                fill_definition(d, config);
                normalize_definition(d);
                d.auto_created = false;
                d.updated_at = now;
                id.to_string()
            }
            None => {
                let alias = new_alias.map(str::trim).unwrap_or_default().to_string();
                validate_alias(&alias)?;
                if let Some(clash) = state
                    .defs
                    .definitions
                    .iter()
                    .find(|d| d.alias.eq_ignore_ascii_case(&alias))
                {
                    return Err(err("definitions.alias_exists")
                        .with("alias", &alias)
                        .with("other", &clash.alias)
                        .into());
                }
                let mut def = ModelDefinition::new(
                    alias,
                    ModelRef::Local {
                        path: PathBuf::new(),
                    },
                );
                // A copy keeps the source's description and default preset.
                if let Some(src) = config.definition_id.as_deref().and_then(|id| state.def(id)) {
                    def.description = src.description.clone();
                    def.default_preset_id = src.default_preset_id.clone();
                }
                def.id = new_id("md");
                def.created_at = now;
                def.updated_at = now;
                fill_definition(&mut def, config);
                normalize_definition(&mut def);
                let id = def.id.clone();
                state.defs.definitions.push(def);
                id
            }
        };
        state.defs_dirty = true;
        state.defs.model_defaults.insert(
            config.model_path.to_string_lossy().to_string(),
            ModelDefault {
                definition_id: def_id.clone(),
                api_key_ref: config.api_key_ref.clone(),
                serve_overrides: serve_part(&config_layer(config)),
            },
        );
        self.commit(&mut state)?;
        Ok(state.def(&def_id).cloned().expect("just saved"))
    }

    /// Effective launch of a form (`config` naming its `definition_id`) on server
    /// `server_id`: the definition (plus the server's profile when it serves the same
    /// definition) with the form's differences as the transient layer. Connection fields come
    /// from the form; the served alias always comes from the stored definition. A form
    /// without a definition (or in router mode) launches as it is.
    pub fn resolve_form(
        &self,
        defaults: &LaunchDefaults,
        server_id: &str,
        config: &LaunchConfig,
    ) -> Result<LaunchConfig, String> {
        let Some(def_id) = config
            .definition_id
            .as_deref()
            .filter(|_| !config.router_mode)
        else {
            let mut plain = config.clone();
            plain.definition_id = None;
            plain.alias = None;
            super::launch::apply_auto_fields(&mut plain);
            return Ok(plain);
        };
        let _g = domain_lock();
        let state = self.state()?;
        let def = state
            .def(def_id)
            .ok_or_else(|| String::from(err("definitions.not_found").with("id", def_id)))?;
        if def.base.local_path() != Some(config.model_path.as_path()) {
            return Err(err("definitions.form_file_mismatch")
                .with("alias", &def.alias)
                .into());
        }
        let profile = state.profiles.profiles.iter().find(|p| {
            p.id == server_id
                && p.id != DEFAULT_SERVER_ID
                && p.definition_id.as_deref() == Some(def_id)
        });
        let base = resolve_launch(defaults, Some(def), profile, &ParamValues::new())?;
        let transient = diff_layer(&base, config);
        let mut out = resolve_launch(defaults, Some(def), profile, &transient)?;
        out.host = config.host.clone();
        out.port = config.port;
        out.port_auto = config.port_auto;
        out.api_key = config.api_key.clone();
        out.api_key_ref = config.api_key_ref.clone();
        Ok(out)
    }

    /// `get_model_preset`: the Dashboard's saved settings for a model file.
    pub fn model_preset(
        &self,
        defaults: &LaunchDefaults,
        model_id: &str,
    ) -> Result<Option<ModelPreset>, String> {
        let _g = domain_lock();
        let state = self.state()?;
        state.model_preset(defaults, model_id)
    }

    /// `save_model_preset` (the API key already moved to the keychain by the caller).
    pub fn save_model_preset(&self, model_id: &str, preset: &ModelPreset) -> Result<(), String> {
        let _g = domain_lock();
        let mut state = self.state()?;
        state.upsert_model_default(model_id, preset, now_secs())?;
        self.commit(&mut state)
    }

    /// A model file moved: its Dashboard default and every definition based on it follow.
    /// Returns the previous API key reference of the Dashboard default.
    pub fn rename_model(
        &self,
        old_id: &str,
        new_id: &str,
        new_api_key_ref: impl FnOnce(Option<&str>) -> Option<String>,
    ) -> Result<(), String> {
        let _g = domain_lock();
        let mut state = self.state()?;
        let old_path = PathBuf::from(old_id);
        let new_path = PathBuf::from(new_id);
        for d in state.defs.definitions.iter_mut() {
            if d.base.local_path() == Some(old_path.as_path()) {
                d.base = ModelRef::Local {
                    path: new_path.clone(),
                };
                state.defs_dirty = true;
            }
            // The file may also be another definition's draft, projector or adapter.
            if let Some(draft) = d.draft.as_mut().filter(|x| x.model == old_path) {
                draft.model = new_path.clone();
                state.defs_dirty = true;
            }
            if d.mmproj.as_deref() == Some(old_path.as_path()) {
                d.mmproj = Some(new_path.clone());
                state.defs_dirty = true;
            }
            for a in d.adapters.iter_mut().filter(|a| a.path == old_path) {
                a.path = new_path.clone();
                state.defs_dirty = true;
            }
        }
        if let Some(mut entry) = state.defs.model_defaults.remove(old_id) {
            entry.api_key_ref = new_api_key_ref(entry.api_key_ref.as_deref());
            state.defs.model_defaults.insert(new_id.to_string(), entry);
            state.defs_dirty = true;
        }
        self.commit(&mut state)
    }

    /// The Dashboard server's last session (`"default"` profile).
    pub fn session(&self, defaults: &LaunchDefaults) -> Result<Option<LaunchConfig>, String> {
        let _g = domain_lock();
        let state = self.state()?;
        if !state
            .profiles
            .profiles
            .iter()
            .any(|p| p.id == DEFAULT_SERVER_ID)
        {
            return Ok(None);
        }
        state
            .resolve_profile(defaults, DEFAULT_SERVER_ID, &ParamValues::new())
            .map(Some)
    }

    /// The Dashboard server as the UI shows it while it is not running (see
    /// [`crate::servers::DefaultServer`]): the saved `"default"` profile, else the settings
    /// defaults. A saved profile that no longer resolves (its definition is gone) still
    /// keeps its address, so the port shown never jumps back to the settings default.
    pub fn default_server(
        &self,
        defaults: &LaunchDefaults,
    ) -> Result<crate::servers::DefaultServer, String> {
        let _g = domain_lock();
        let state = self.state()?;
        let fallback = || resolve_launch(defaults, None, None, &ParamValues::new());
        let Some(p) = state
            .profiles
            .profiles
            .iter()
            .find(|p| p.id == DEFAULT_SERVER_ID)
        else {
            return Ok(crate::servers::DefaultServer {
                config: fallback()?,
                saved: false,
            });
        };
        Ok(
            match state.resolve_profile(defaults, DEFAULT_SERVER_ID, &ParamValues::new()) {
                Ok(config) => crate::servers::DefaultServer {
                    config,
                    saved: true,
                },
                Err(_) => {
                    let mut config = fallback()?;
                    config.host = p.host.clone();
                    config.port = p.port;
                    config.port_auto = p.port_auto;
                    crate::servers::DefaultServer {
                        config,
                        saved: false,
                    }
                }
            },
        )
    }

    /// Remembers the Dashboard server's launch (`api_key_ref` already set by the caller).
    pub fn save_session(&self, config: &LaunchConfig) -> Result<(), String> {
        let _g = domain_lock();
        let mut state = self.state()?;
        state.set_session(config, now_secs())?;
        self.commit(&mut state)
    }

    pub fn clear_session(&self) -> Result<(), String> {
        self.delete_profile(DEFAULT_SERVER_ID)
    }

    // --- inference presets ---

    fn user_presets_locked(&self) -> Result<Vec<InferencePreset>, String> {
        Ok(self
            .presets_store()
            .load_versioned_or_default::<PresetsFile>(&PRESETS_SCHEMA)?
            .data
            .presets)
    }

    /// Built-ins first, then the user's presets.
    pub fn inference_presets(&self) -> Result<Vec<InferencePreset>, String> {
        let _g = domain_lock();
        let mut all = builtin_presets();
        all.extend(self.user_presets_locked()?);
        Ok(all)
    }

    pub fn inference_preset(&self, id: &str) -> Result<InferencePreset, String> {
        self.inference_presets()?
            .into_iter()
            .find(|p| p.id == id)
            .ok_or_else(|| String::from(err("presets.not_found").with("id", id)))
    }

    /// Creates (empty `id`) or replaces a user preset. Built-ins are read-only.
    pub fn save_inference_preset(
        &self,
        mut preset: InferencePreset,
    ) -> Result<InferencePreset, String> {
        let _g = domain_lock();
        if is_builtin_id(&preset.id) || preset.builtin {
            return Err(err("presets.builtin_read_only").into());
        }
        preset.name = preset.name.trim().to_string();
        preset.validate()?;
        let now = now_secs();
        preset.updated_at = now;
        self.presets_store()
            .update_versioned::<PresetsFile, _>(&PRESETS_SCHEMA, |file| {
                match file.presets.iter_mut().find(|p| p.id == preset.id) {
                    Some(existing) if !preset.id.is_empty() => {
                        preset.created_at = existing.created_at;
                        *existing = preset.clone();
                    }
                    _ => {
                        if preset.id.is_empty() {
                            preset.id = new_id("ip");
                        }
                        preset.created_at = now;
                        file.presets.push(preset.clone());
                    }
                }
                Ok(preset.clone())
            })
    }

    // --- portable definitions (*.harness.toml) ---

    /// `*.harness.toml` text of a definition; `embed_preset` includes its default preset
    /// when that is a user preset (built-ins are referenced by id).
    pub fn export_definition_toml(
        &self,
        id: &str,
        embed_preset: bool,
        models_dirs: &[PathBuf],
    ) -> Result<String, String> {
        let def = self.definition(id)?;
        let preset = match (embed_preset, def.default_preset_id.as_deref()) {
            (true, Some(p)) => self.inference_preset(p).ok(),
            _ => None,
        };
        super::portable::export_definition(&def, preset.as_ref(), models_dirs)
    }

    /// Reads a `*.harness.toml` without saving anything (see [`Self::import_definition`]).
    pub fn preview_definition_import(
        &self,
        text: &str,
        models_dirs: &[PathBuf],
    ) -> Result<super::portable::DefinitionImport, String> {
        let taken: Vec<String> = self.definitions()?.into_iter().map(|d| d.alias).collect();
        super::portable::parse_definition(text, models_dirs, &taken)
    }

    /// Saves a previewed import under `alias` (its embedded preset becomes a new user preset
    /// and the definition's default).
    pub fn import_definition(
        &self,
        import: super::portable::DefinitionImport,
        alias: &str,
    ) -> Result<ModelDefinition, String> {
        let mut def = import.definition;
        def.id = String::new();
        def.alias = alias.trim().to_string();
        validate_alias(&def.alias)?;
        if self
            .definitions()?
            .iter()
            .any(|d| d.alias.eq_ignore_ascii_case(&def.alias))
        {
            return Err(err("definitions.alias_already_exists")
                .with("alias", &def.alias)
                .into());
        }
        if let Some(p) = import.preset {
            let text = super::presets::export_presets(&[p]);
            if let Some(saved) = self.import_inference_presets(&text)?.into_iter().next() {
                def.default_preset_id = Some(saved.id);
            }
        }
        if let Some(text) = import.chat_template_text.as_deref() {
            let saved = crate::chat_template::save_template_unique(
                &self.chat_templates_dir(),
                &def.alias,
                text,
            )?;
            def.load.insert(
                crate::chat_template::PARAM_ID.to_string(),
                ParamValue::Text(saved.path.to_string_lossy().to_string()),
            );
        }
        self.save_definition(def)
    }

    /// Where the template editor keeps custom chat templates.
    pub fn chat_templates_dir(&self) -> PathBuf {
        self.dir.join("chat-templates")
    }

    /// `harness.preset.v1` JSON of the given presets (`None` = every user preset).
    pub fn export_inference_presets(&self, ids: Option<&[String]>) -> Result<String, String> {
        let all = self.inference_presets()?;
        let chosen: Vec<InferencePreset> =
            match ids {
                Some(ids) => {
                    let mut out = Vec::new();
                    for id in ids {
                        out.push(all.iter().find(|p| &p.id == id).cloned().ok_or_else(|| {
                            String::from(err("presets.not_found").with("id", id))
                        })?);
                    }
                    out
                }
                None => all.into_iter().filter(|p| !p.builtin).collect(),
            };
        if chosen.is_empty() {
            return Err(err("presets.nothing_to_export").into());
        }
        Ok(super::presets::export_presets(&chosen))
    }

    /// Adds every preset of an exported file as a new user preset (names that are taken get
    /// a ` (2)` suffix). Nothing is saved when one of them is invalid.
    pub fn import_inference_presets(&self, text: &str) -> Result<Vec<InferencePreset>, String> {
        let parsed = super::presets::parse_presets(text)?;
        let mut names: Vec<String> = self
            .inference_presets()?
            .into_iter()
            .map(|p| p.name.to_lowercase())
            .collect();
        let mut saved = Vec::new();
        for mut p in parsed {
            let base = p.name.clone();
            let mut n = 2;
            while names.contains(&p.name.to_lowercase()) {
                p.name = format!("{} ({})", base, n);
                n += 1;
            }
            names.push(p.name.to_lowercase());
            saved.push(self.save_inference_preset(p)?);
        }
        Ok(saved)
    }

    pub fn delete_inference_preset(&self, id: &str) -> Result<(), String> {
        let _g = domain_lock();
        if is_builtin_id(id) {
            return Err(err("presets.builtin_undeletable").into());
        }
        self.presets_store()
            .update_versioned::<PresetsFile, _>(&PRESETS_SCHEMA, |file| {
                let before = file.presets.len();
                file.presets.retain(|p| p.id != id);
                if before == file.presets.len() {
                    Err(String::from(err("presets.not_found").with("id", id)))
                } else {
                    Ok(())
                }
            })?;
        // Definitions that defaulted to it fall back to none.
        let mut state = self.state()?;
        for d in state.defs.definitions.iter_mut() {
            if d.default_preset_id.as_deref() == Some(id) {
                d.default_preset_id = None;
                state.defs_dirty = true;
            }
        }
        self.commit(&mut state)
    }
}

impl State {
    fn def(&self, id: &str) -> Option<&ModelDefinition> {
        self.defs.definitions.iter().find(|d| d.id == id)
    }

    fn taken_aliases(&self) -> Vec<String> {
        self.defs
            .definitions
            .iter()
            .map(|d| d.alias.clone())
            .collect()
    }

    /// User profiles (not the Dashboard session) serving `def_id`.
    fn users_of(&self, def_id: &str) -> Vec<String> {
        self.profiles
            .profiles
            .iter()
            .filter(|p| p.id != DEFAULT_SERVER_ID && p.definition_id.as_deref() == Some(def_id))
            .map(|p| p.id.clone())
            .collect()
    }

    fn is_model_default(&self, def_id: &str) -> bool {
        self.defs
            .model_defaults
            .values()
            .any(|d| d.definition_id == def_id)
    }

    fn new_definition(&mut self, alias_base: &str, config: &LaunchConfig, now: u64) -> String {
        let taken = self.taken_aliases();
        let mut def = ModelDefinition {
            id: new_id("md"),
            alias: unique_alias(alias_base, taken.iter().map(|s| s.as_str())),
            base: ModelRef::Local {
                path: PathBuf::new(),
            },
            description: String::new(),
            mmproj: None,
            adapters: Vec::new(),
            draft: None,
            chat_template: None,
            chat_template_kwargs: None,
            extra_args: None,
            load: ParamValues::new(),
            default_preset_id: None,
            ttl_seconds: None,
            pinned: false,
            capture_content: false,
            content_retention_days: None,
            auto_created: true,
            created_at: now,
            updated_at: now,
        };
        fill_definition(&mut def, config);
        let id = def.id.clone();
        self.defs.definitions.push(def);
        self.defs_dirty = true;
        id
    }

    /// A definition with the same base file and the same model-level settings.
    fn identical_definition(
        &self,
        config: &LaunchConfig,
        prefer: &[Option<&str>],
    ) -> Option<String> {
        let target = model_part(&config_layer(config));
        let same = |d: &ModelDefinition| {
            d.base.local_path() == Some(config.model_path.as_path())
                && super::launch::definition_layer(d) == target
        };
        for id in prefer.iter().flatten() {
            if self.def(id).is_some_and(same) {
                return Some(id.to_string());
            }
        }
        self.defs
            .definitions
            .iter()
            .find(|d| same(d))
            .map(|d| d.id.clone())
    }

    /// Dashboard settings for a model file (old `presets.json` entry).
    fn upsert_model_default(
        &mut self,
        model_id: &str,
        preset: &ModelPreset,
        now: u64,
    ) -> Result<(), String> {
        let config = preset.to_launch_config(Path::new(model_id));
        let layer = config_layer(&config);
        // No Dashboard entry yet: the file's first definition, the one the Dashboard shows (a
        // download already made one), instead of a second `<stem>-2` definition on first start.
        let path = Path::new(model_id);
        let existing = self
            .defs
            .model_defaults
            .get(model_id)
            .map(|d| d.definition_id.clone())
            .filter(|id| self.def(id).is_some())
            .or_else(|| {
                self.defs
                    .definitions
                    .iter()
                    .find(|d| d.base.local_path() == Some(path))
                    .map(|d| d.id.clone())
            });
        let def_id = match existing {
            Some(id) => {
                // Copy-on-write: profiles that shared this definition keep their settings.
                let unchanged = self
                    .def(&id)
                    .is_some_and(|d| super::launch::definition_layer(d) == model_part(&layer));
                if !unchanged {
                    for profile_id in self.users_of(&id) {
                        let old = self.def(&id).cloned().expect("checked above");
                        let name = self
                            .profiles
                            .profiles
                            .iter()
                            .find(|p| p.id == profile_id)
                            .map(|p| p.name.clone())
                            .unwrap_or_else(|| profile_id.clone());
                        let taken = self.taken_aliases();
                        let mut fork = old;
                        fork.id = new_id("md");
                        fork.alias = unique_alias(
                            &profile_alias_base(&stem_alias(model_id), &name),
                            taken.iter().map(|s| s.as_str()),
                        );
                        fork.created_at = now;
                        fork.updated_at = now;
                        fork.auto_created = true;
                        let fork_id = fork.id.clone();
                        self.defs.definitions.push(fork);
                        if let Some(p) = self
                            .profiles
                            .profiles
                            .iter_mut()
                            .find(|p| p.id == profile_id)
                        {
                            p.definition_id = Some(fork_id);
                        }
                        self.profiles_dirty = true;
                    }
                    let d = self
                        .defs
                        .definitions
                        .iter_mut()
                        .find(|d| d.id == id)
                        .expect("checked above");
                    fill_definition(d, &config);
                    d.updated_at = now;
                    self.defs_dirty = true;
                }
                id
            }
            None => self.new_definition(&stem_alias(model_id), &config, now),
        };
        let entry = ModelDefault {
            definition_id: def_id,
            api_key_ref: preset.api_key_ref.clone(),
            serve_overrides: serve_part(&layer),
        };
        if self.defs.model_defaults.get(model_id) != Some(&entry) {
            self.defs.model_defaults.insert(model_id.to_string(), entry);
            self.defs_dirty = true;
        }
        Ok(())
    }

    fn model_preset(
        &self,
        defaults: &LaunchDefaults,
        model_id: &str,
    ) -> Result<Option<ModelPreset>, String> {
        let Some(entry) = self.defs.model_defaults.get(model_id) else {
            return Ok(None);
        };
        let Some(def) = self.def(&entry.definition_id) else {
            return Ok(None);
        };
        let mut config = resolve_launch(defaults, Some(def), None, &entry.serve_overrides)?;
        config.api_key_ref = entry.api_key_ref.clone();
        Ok(Some(ModelPreset::from_launch_config(&config)))
    }

    /// A profile from the old `{config}` shape: reuses an identical definition (the profile's
    /// own, the model's Dashboard default, or any other), updates the profile's own
    /// definition in place when nothing else uses it, and creates `<stem>-<name>` otherwise.
    fn upsert_profile_view(&mut self, view: &ProfileView, now: u64) -> Result<(), String> {
        let config = &view.config;
        let layer = config_layer(config);
        let current = self
            .profiles
            .profiles
            .iter()
            .find(|p| p.id == view.id)
            .cloned();
        let (definition_id, router, serve_overrides) = if config.router_mode {
            (
                None,
                Some(RouterSpec {
                    models_dir: config.models_path.clone(),
                }),
                layer,
            )
        } else if let Some(def) = config
            .definition_id
            .as_deref()
            .and_then(|id| self.def(id))
            .filter(|d| d.base.local_path() == Some(config.model_path.as_path()))
            // A definition Harness Studio made for this profile alone keeps following the
            // profile's edits (updated in place below).
            .filter(|d| {
                !(d.auto_created
                    && !self.is_model_default(&d.id)
                    && self.users_of(&d.id).iter().all(|u| u == &view.id))
            })
        {
            // The form names its definition: the profile references it and keeps only what
            // differs (later edits of the definition reach the profile).
            let base = resolve_launch(
                &LaunchDefaults::from_config(config),
                Some(def),
                None,
                &ParamValues::new(),
            )?;
            let mut overrides = diff_layer(&base, config);
            // Required numbers the definition leaves to the app defaults are pinned, so a later
            // change of the defaults doesn't change the profile.
            let def_layer = super::launch::definition_layer(def);
            for (id, v) in config_layer(config) {
                if matches!(id.as_str(), "gpu_layers" | "context_size")
                    && !def_layer.contains_key(&id)
                {
                    overrides.insert(id, v);
                }
            }
            (Some(def.id.clone()), None, overrides)
        } else {
            let own = current.as_ref().and_then(|p| p.definition_id.clone());
            let model_default = self
                .defs
                .model_defaults
                .get(&config.model_path.to_string_lossy().to_string())
                .map(|d| d.definition_id.clone());
            let def_id = match self
                .identical_definition(config, &[own.as_deref(), model_default.as_deref()])
            {
                Some(id) => id,
                None => match own.filter(|id| {
                    self.def(id).is_some()
                        && !self.is_model_default(id)
                        && self.users_of(id).iter().all(|u| u == &view.id)
                }) {
                    Some(id) => {
                        let d = self
                            .defs
                            .definitions
                            .iter_mut()
                            .find(|d| d.id == id)
                            .expect("checked");
                        fill_definition(d, config);
                        d.updated_at = now;
                        self.defs_dirty = true;
                        id
                    }
                    None => {
                        let stem = stem_alias(&config.model_path.to_string_lossy());
                        self.new_definition(&profile_alias_base(&stem, &view.name), config, now)
                    }
                },
            };
            (Some(def_id), None, serve_part(&layer))
        };
        let profile = ServerProfile {
            id: view.id.clone(),
            name: view.name.clone(),
            purpose: view.description.clone(),
            definition_id,
            router,
            host: config.host.clone(),
            port: config.port,
            port_auto: config.wants_auto_port(),
            auto_start: view.auto_start,
            api_key_ref: config.api_key_ref.clone(),
            serve_overrides,
            created_at: current.as_ref().map_or(now, |p| p.created_at),
            updated_at: now,
        };
        let previous = current.and_then(|p| p.definition_id);
        match self.profiles.profiles.iter_mut().find(|p| p.id == view.id) {
            Some(existing) => *existing = profile,
            None => self.profiles.profiles.push(profile),
        }
        self.profiles_dirty = true;
        if let Some(old) = previous {
            self.drop_orphan_auto(&old);
        }
        Ok(())
    }

    /// Removes an `auto_created` definition nothing references any more (no profile, not a
    /// Dashboard default). User-created definitions are never removed here.
    fn drop_orphan_auto(&mut self, def_id: &str) {
        let referenced = self
            .profiles
            .profiles
            .iter()
            .any(|p| p.definition_id.as_deref() == Some(def_id))
            || self.is_model_default(def_id);
        if referenced || !self.def(def_id).is_some_and(|d| d.auto_created) {
            return;
        }
        self.defs.definitions.retain(|d| d.id != def_id);
        self.defs_dirty = true;
    }

    /// Dashboard session: points at the model's Dashboard definition (created if the model has
    /// none) and pins the exact launch with snapshot overrides.
    fn set_session(&mut self, config: &LaunchConfig, now: u64) -> Result<(), String> {
        let (definition_id, router, serve_overrides) = if config.router_mode {
            (
                None,
                Some(RouterSpec {
                    models_dir: config.models_path.clone(),
                }),
                config_layer(config),
            )
        } else {
            let key = config.model_path.to_string_lossy().to_string();
            let named = config.definition_id.clone().filter(|id| {
                self.def(id)
                    .is_some_and(|d| d.base.local_path() == Some(config.model_path.as_path()))
            });
            let def_id = match named.or_else(|| {
                self.defs
                    .model_defaults
                    .get(&key)
                    .map(|d| d.definition_id.clone())
                    .filter(|id| self.def(id).is_some())
            }) {
                Some(id) => id,
                None => {
                    let preset = ModelPreset::from_launch_config(&LaunchConfig {
                        api_key_ref: None,
                        ..config.clone()
                    });
                    self.upsert_model_default(&key, &preset, now)?;
                    let id = self.defs.model_defaults[&key].definition_id.clone();
                    // The layer count of this start is the estimate's, not a choice: the
                    // definition leaves it automatic.
                    if config.gpu_layers_auto {
                        if let Some(d) = self.defs.definitions.iter_mut().find(|d| d.id == id) {
                            d.load.remove("gpu_layers");
                        }
                    }
                    id
                }
            };
            let def = self.def(&def_id).cloned();
            (Some(def_id), None, snapshot_overrides(def.as_ref(), config))
        };
        let created_at = self
            .profiles
            .profiles
            .iter()
            .find(|p| p.id == DEFAULT_SERVER_ID)
            .map_or(now, |p| p.created_at);
        let profile = ServerProfile {
            id: DEFAULT_SERVER_ID.to_string(),
            name: "Dashboard".to_string(),
            purpose: String::new(),
            definition_id,
            router,
            host: config.host.clone(),
            port: config.port,
            port_auto: config.wants_auto_port(),
            auto_start: false,
            api_key_ref: config.api_key_ref.clone(),
            serve_overrides,
            created_at,
            updated_at: now,
        };
        match self
            .profiles
            .profiles
            .iter_mut()
            .find(|p| p.id == DEFAULT_SERVER_ID)
        {
            Some(existing) => *existing = profile,
            None => self.profiles.profiles.insert(0, profile),
        }
        self.profiles_dirty = true;
        Ok(())
    }

    fn resolve_profile(
        &self,
        defaults: &LaunchDefaults,
        profile_id: &str,
        transient: &ParamValues,
    ) -> Result<LaunchConfig, String> {
        let p = self
            .profiles
            .profiles
            .iter()
            .find(|p| p.id == profile_id)
            .ok_or_else(|| String::from(err("profiles.not_found").with("id", profile_id)))?;
        let def = match p.definition_id.as_deref() {
            Some(id) => Some(
                self.def(id)
                    .ok_or_else(|| String::from(err("definitions.not_found").with("id", id)))?,
            ),
            None => None,
        };
        resolve_launch(defaults, def, Some(p), transient)
    }

    fn view(&self, defaults: &LaunchDefaults, p: &ServerProfile) -> Result<ProfileView, String> {
        let config = self.resolve_profile(defaults, &p.id, &ParamValues::new())?;
        Ok(ProfileView {
            id: p.id.clone(),
            name: p.name.clone(),
            model_id: (!config.router_mode)
                .then(|| config.model_path.to_string_lossy().to_string()),
            description: p.purpose.clone(),
            config,
            auto_start: p.auto_start,
            updated_at: p.updated_at,
        })
    }
}

/// Alias base for a model file: the name clients saw so far (`--alias` = file stem without
/// the split suffix).
fn stem_alias(model_id: &str) -> String {
    let stem = crate::process::model_alias(Path::new(model_id)).unwrap_or_else(|| "model".into());
    sanitize_alias(&stem)
}

/// Alias base for a definition created for a named server: the file stem, plus the server name
/// when it says something more. The auto-generated name (`<stem> :8082`) carries the port and
/// the stem itself, so both are dropped: the served id stays a clean, stable `<stem>`.
pub(crate) fn profile_alias_base(stem: &str, server_name: &str) -> String {
    let name = server_name.trim();
    let name = match name.rsplit_once(':') {
        Some((head, port)) if !port.is_empty() && port.chars().all(|c| c.is_ascii_digit()) => head,
        _ => name,
    };
    let name = if name.trim().is_empty() {
        String::new()
    } else {
        sanitize_alias(&name.replace(':', "-"))
    };
    let stem_s = sanitize_alias(stem);
    let lower = name.to_ascii_lowercase();
    if name.is_empty() || lower == stem_s.to_ascii_lowercase() {
        stem_s
    } else if lower.starts_with(&stem_s.to_ascii_lowercase()) {
        name
    } else {
        sanitize_alias(&format!("{}-{}", stem_s, name))
    }
}

/// `model_definitions.json` v1 -> v2: repairs the served ids builds before the clean-alias fix
/// generated for a new server. They appended the server's auto name (`<model> :<port>`) to the
/// file stem, so the alias repeated the stem and carried the port
/// (`nomic-embed-text-v1.5.Q8_0-nomic-embed-text-v1.5.Q8_0-:8082`). Only aliases of exactly
/// that shape (see [`repaired_auto_alias`]) are rewritten; every other alias, user-chosen or
/// not, is left as it is. The framework keeps the v1 file as `model_definitions.json.bak-v1`.
fn definitions_v1_to_v2(mut doc: serde_json::Value) -> Result<serde_json::Value, String> {
    let Some(list) = doc.get_mut("definitions").and_then(|d| d.as_array_mut()) else {
        return Ok(doc);
    };
    let mut aliases: Vec<String> = list
        .iter()
        .map(|d| d["alias"].as_str().unwrap_or_default().to_string())
        .collect();
    for (i, def) in list.iter_mut().enumerate() {
        let local = def["base"]["kind"].as_str() == Some("local");
        let (Some(path), true) = (def["base"]["path"].as_str(), local) else {
            continue;
        };
        let Some(base) = repaired_auto_alias(&stem_alias(path), &aliases[i]) else {
            continue;
        };
        let alias = unique_alias(
            &base,
            aliases
                .iter()
                .enumerate()
                .filter(|(j, _)| *j != i)
                .map(|(_, a)| a.as_str()),
        );
        def["alias"] = serde_json::Value::String(alias.clone());
        aliases[i] = alias;
    }
    Ok(doc)
}

/// `server_profiles.json` v1 -> v2: every start of the Dashboard server (profile `default`, the
/// "last session") used to write the form's numbers into its `serve_overrides`, among them the
/// context, layer count and slot count the form showed from the estimate or the default-load
/// rule. The next start then saw them as chosen values and the rule never applied again. The
/// Dashboard server is rewritten from the form at every start (it is the last session, not a
/// profile anyone saved by name), so its `context_size`, `gpu_layers`, `parallel_slots` and cache types go
/// back to automatic; named profiles are left alone. The framework keeps the v1 file as
/// `server_profiles.json.bak-v1`.
fn profiles_v1_to_v2(mut doc: serde_json::Value) -> Result<serde_json::Value, String> {
    let Some(list) = doc.get_mut("profiles").and_then(|p| p.as_array_mut()) else {
        return Ok(doc);
    };
    for p in list
        .iter_mut()
        .filter(|p| p["id"].as_str() == Some(DEFAULT_SERVER_ID))
    {
        if let Some(o) = p.get_mut("serve_overrides").and_then(|o| o.as_object_mut()) {
            for key in [
                "context_size",
                "gpu_layers",
                "parallel_slots",
                "cache_type_k",
                "cache_type_v",
            ] {
                o.remove(key);
            }
        }
    }
    Ok(doc)
}

/// `model_definitions.json` v2 -> v3: older builds wrote the numbers their auto-fit chose
/// (`context_size`, `gpu_layers`) into the definition they made themselves for a model, so the
/// automatic full-GPU rule ([`crate::full_offload`]) never applied to upgraded users. This
/// sets those two back to automatic (removes them from `load`) for a definition only when all
/// of it shows Harness Studio made it and nobody shaped it since:
/// * `auto_created` is still true (every user save clears it) and it is a model's Dashboard
///   default (`model_defaults`), not a definition made for a named server;
/// * its alias is the plain file stem (`<stem>` or `<stem>-<n>`), it has no description, chat
///   template, template kwargs, custom arguments, projector, adapter, draft model, preset,
///   TTL, pin or content capture;
/// * no cache type or slot count is set either (those would block the rule anyway, and a
///   hand-picked one marks a user's definition).
///
/// Everything else (other `load` values included) stays. The framework keeps the v2 file as
/// `model_definitions.json.bak-v2`.
fn definitions_v2_to_v3(mut doc: serde_json::Value) -> Result<serde_json::Value, String> {
    use serde_json::Value;
    let defaults: std::collections::HashSet<String> = doc
        .get("model_defaults")
        .and_then(Value::as_object)
        .map(|m| {
            m.values()
                .filter_map(|d| d["definition_id"].as_str().map(str::to_string))
                .collect()
        })
        .unwrap_or_default();
    let Some(list) = doc.get_mut("definitions").and_then(Value::as_array_mut) else {
        return Ok(doc);
    };
    let blank = |v: &Value| match v {
        Value::Null => true,
        Value::String(s) => s.trim().is_empty(),
        Value::Array(a) => a.is_empty(),
        _ => false,
    };
    for def in list.iter_mut() {
        let local_path = (def["base"]["kind"].as_str() == Some("local"))
            .then(|| def["base"]["path"].as_str())
            .flatten();
        let (Some(path), Some(id)) = (local_path, def["id"].as_str()) else {
            continue;
        };
        let stem = stem_alias(path);
        let alias = def["alias"].as_str().unwrap_or_default();
        let plain_alias = alias == stem
            || alias
                .strip_prefix(stem.as_str())
                .and_then(|r| r.strip_prefix('-'))
                .is_some_and(|n| !n.is_empty() && n.chars().all(|c| c.is_ascii_digit()));
        let untouched = [
            "description",
            "mmproj",
            "adapters",
            "draft",
            "chat_template",
            "chat_template_kwargs",
            "extra_args",
            "default_preset_id",
            "ttl_seconds",
        ]
        .iter()
        .all(|k| blank(&def[*k]))
            && def["pinned"].as_bool() != Some(true)
            && def["capture_content"].as_bool() != Some(true)
            && def["content_retention_days"].is_null();
        let load_clear = ["cache_type_k", "cache_type_v", "parallel_slots"]
            .iter()
            .all(|k| blank(&def["load"][*k]));
        if def["auto_created"].as_bool() != Some(true)
            || !defaults.contains(id)
            || !plain_alias
            || !untouched
            || !load_clear
        {
            continue;
        }
        if let Some(load) = def.get_mut("load").and_then(Value::as_object_mut) {
            load.remove("context_size");
            load.remove("gpu_layers");
        }
    }
    Ok(doc)
}

/// The clean alias base for an alias generated by the old new-server code, `None` for anything
/// else. The old alias was `<stem>-<auto name>` with the auto name `<model> :<port>` sanitized
/// to `<model>-:<port>`, and a `-<n>` suffix when that was taken: exactly
/// `<stem>-<model>-:<digits>` or `<stem>-<model>-:<digits>-<digits>`.
fn repaired_auto_alias(stem: &str, alias: &str) -> Option<String> {
    let rest = alias.strip_prefix(stem)?.strip_prefix('-')?;
    let (model, tail) = rest.rsplit_once("-:")?;
    let digits = |s: &str| !s.is_empty() && s.chars().all(|c| c.is_ascii_digit());
    let port_ok = match tail.split_once('-') {
        Some((port, n)) => digits(port) && digits(n),
        None => digits(tail),
    };
    if model.is_empty() || !port_ok {
        return None;
    }
    let clean = profile_alias_base(stem, model);
    (clean != alias).then_some(clean)
}

fn read_json(path: &Path) -> Result<serde_json::Value, String> {
    let text = std::fs::read_to_string(path).map_err(|e| {
        String::from(
            err("fs.read_failed")
                .with("path", path.display())
                .with("error", e),
        )
    })?;
    serde_json::from_str(&text).map_err(|e| {
        String::from(
            err("json.parse_failed")
                .with("path", path.display())
                .with("error", e),
        )
    })
}

/// Removes plaintext `api_key` values from a legacy document (presets map, profile list or
/// launch config).
fn scrub_keys(doc: &mut serde_json::Value) {
    match doc {
        serde_json::Value::Object(map) => {
            map.remove("api_key");
            for v in map.values_mut() {
                scrub_keys(v);
            }
        }
        serde_json::Value::Array(list) => list.iter_mut().for_each(scrub_keys),
        _ => {}
    }
}

/// Moves a legacy file to `<name>.pre-v2` (or `.pre-v2-<n>` when one already exists).
/// With `scrub` the backup is written without plaintext API keys (they are in the keychain).
fn retire_legacy(path: &Path, scrub: bool) -> Result<PathBuf, String> {
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_default();
    let mut backup = path.with_file_name(format!("{}.pre-v2", name));
    let mut n = 2;
    while backup.exists() {
        backup = path.with_file_name(format!("{}.pre-v2-{}", name, n));
        n += 1;
    }
    let content = std::fs::read(path).map_err(|e| {
        String::from(
            err("fs.read_failed")
                .with("path", path.display())
                .with("error", e),
        )
    })?;
    let has_plaintext = scrub
        && serde_json::from_slice::<serde_json::Value>(&content)
            .map(|v| v.to_string().contains("\"api_key\":\""))
            .unwrap_or(false);
    if has_plaintext {
        let mut doc: serde_json::Value = serde_json::from_slice(&content).map_err(|e| {
            String::from(
                err("json.parse_failed")
                    .with("path", path.display())
                    .with("error", e),
            )
        })?;
        scrub_keys(&mut doc);
        let bytes = serde_json::to_vec_pretty(&doc)
            .map_err(|e| String::from(err("json.serialize_failed").with("error", e)))?;
        JsonStore::sensitive(&backup).write_bytes(&bytes)?;
        std::fs::remove_file(path).map_err(|e| {
            String::from(
                err("fs.remove_failed")
                    .with("path", path.display())
                    .with("error", e),
            )
        })?;
    } else {
        std::fs::rename(path, &backup).map_err(|e| {
            String::from(
                err("fs.rename_failed")
                    .with("from", path.display())
                    .with("to", backup.display())
                    .with("error", e),
            )
        })?;
    }
    Ok(backup)
}

/// Version state of one data file (Settings > diagnostics, support).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct DataFileStatus {
    pub file: String,
    /// `None`: the file doesn't exist (yet).
    pub version: Option<u32>,
    pub supported: u32,
    /// Written by a newer Harness Studio: read-only for this build.
    pub read_only: bool,
}

/// Every versioned JSON file of the config directory.
pub fn all_schemas() -> Vec<Schema> {
    vec![
        crate::config::SETTINGS_SCHEMA,
        DEFINITIONS_SCHEMA,
        PROFILES_SCHEMA,
        PRESETS_SCHEMA,
        crate::mcp::SCHEMA,
        crate::capabilities::FILE_SCHEMA,
        crate::downloader::QUEUE_SCHEMA,
        crate::downloader::HASHES_SCHEMA,
        crate::library::sources::SOURCES_SCHEMA,
        crate::benchmark::HISTORY_SCHEMA,
        crate::importer::EXTERNAL_SCHEMA,
    ]
}

pub fn data_files_status(dir: &Path) -> Vec<DataFileStatus> {
    all_schemas()
        .into_iter()
        .map(|s| {
            let version = std::fs::read_to_string(dir.join(s.file))
                .ok()
                .and_then(|t| serde_json::from_str::<serde_json::Value>(&t).ok())
                .map(|d| crate::store::document_version(&d));
            DataFileStatus {
                file: s.file.to_string(),
                version,
                supported: s.current,
                read_only: version.is_some_and(|v| v > s.current),
            }
        })
        .collect()
}

#[cfg(test)]
#[path = "store_tests.rs"]
mod tests;
