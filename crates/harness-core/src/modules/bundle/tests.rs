use super::*;
use std::sync::Mutex as StdMutex;

const APP: &str = "9.9.9";

fn host() -> String {
    format!("{}-{}", std::env::consts::OS, std::env::consts::ARCH)
}
/// The bundled speech manifest, made installable on whatever machine runs the tests.
fn manifest() -> ModuleManifest {
    let mut m = bundled().remove(0);
    m.platforms = vec![host()];
    m.min_app_version = "0.0.0".into();
    m
}
fn layout(dir: &Path) -> Layout {
    Layout {
        base: dir.join("bin"),
        store_file: dir.join("config/modules.json"),
    }
}
fn write(path: &Path, text: &str) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, text).unwrap();
}
fn python_dir() -> String {
    format!("cpython-{}.1-test", pyenv::PYTHON_SERIES)
}
/// The files of an installed speech module, `tag` makes the content recognizable.
fn populate(layout: &Layout, tag: &str) {
    let py = layout.pyroot();
    let env = py.join("envs").join(ENV);
    write(
        &env.join("manifest.json"),
        &format!("{{\"tag\":\"{tag}\"}}"),
    );
    write(
        &env.join("venv/pyvenv.cfg"),
        &format!("home = {}\nversion = 3.12.1\n", py.join("python").display()),
    );
    write(&env.join("venv/lib/pkg/__init__.py"), &format!("# {tag}"));
    write(
        &py.join("uv").join(pyenv::UV_VERSION).join("uv.bin"),
        &format!("uv {tag}"),
    );
    write(
        &py.join("python").join(python_dir()).join("python.bin"),
        &format!("python {tag}"),
    );
}
fn install_fixture(layout: &Layout, m: &ModuleManifest, tag: &str) {
    populate(layout, tag);
    let mut data = layout.load().unwrap();
    data.claim(m);
    layout.store().save(&data).unwrap();
}
fn ok(_: &str) -> Result<(), String> {
    Ok(())
}
fn go(
    layout: &Layout,
    bundle: &Path,
    index: &[OfficialBundle],
    m: &ModuleManifest,
    verify: &dyn Fn(&str) -> Result<(), String>,
    after: &dyn Fn(&str) -> Result<(), String>,
) -> Result<ImportReport, String> {
    let resolve = |_: &str| Ok(m.clone());
    let before = |_: &str, _: bool| Ok(());
    import_at(
        layout,
        bundle,
        index,
        &host(),
        APP,
        &Hooks {
            resolve: &resolve,
            before_commit: &before,
            verify,
            after_move: after,
        },
        &StdMutex::new(()),
    )
}
fn official(m: &ModuleManifest, sha: &str) -> Vec<OfficialBundle> {
    vec![OfficialBundle {
        id: m.id.clone(),
        version: m.version.clone(),
        sha256: sha.into(),
    }]
}
fn export_of(layout: &Layout, m: &ModuleManifest, out: &Path) -> ExportReport {
    let data = layout.load().unwrap();
    export_at(layout, m, &data, &host(), APP, out).unwrap()
}
/// Rewrites a bundle, letting `edit` change any entry (name, bytes).
fn rewrite(src: &Path, dst: &Path, edit: &dyn Fn(&str, Vec<u8>) -> Vec<u8>) -> String {
    let mut reader = zip::ZipArchive::new(std::fs::File::open(src).unwrap()).unwrap();
    let mut writer = zip::ZipWriter::new(std::fs::File::create(dst).unwrap());
    for i in 0..reader.len() {
        let mut entry = reader.by_index(i).unwrap();
        let name = entry.name().unwrap().to_string();
        let mut bytes = Vec::new();
        entry.read_to_end(&mut bytes).unwrap();
        writer
            .start_file(&name, zip::write::SimpleFileOptions::default())
            .unwrap();
        writer.write_all(&edit(&name, bytes)).unwrap();
    }
    writer.finish().unwrap();
    sha256_file(dst).unwrap()
}
fn tree(dir: &Path) -> Vec<String> {
    fn walk(dir: &Path, out: &mut Vec<String>) {
        for e in std::fs::read_dir(dir).into_iter().flatten().flatten() {
            let p = e.path();
            if p.is_dir() {
                walk(&p, out);
            } else {
                out.push(format!(
                    "{}={}",
                    p.display(),
                    std::fs::read_to_string(&p).unwrap_or_default()
                ));
            }
        }
    }
    let mut out = Vec::new();
    walk(dir, &mut out);
    out.sort();
    out
}

#[test]
fn export_then_import_round_trips_into_another_data_directory() {
    let m = manifest();
    let (a, b) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let (src, dst) = (layout(a.path()), layout(b.path()));
    install_fixture(&src, &m, "one");
    let bundle = a.path().join("speech.hsmod");
    let report = export_of(&src, &m, &bundle);
    assert_eq!(report.id, "speech");
    assert_eq!(report.files, 5);
    assert_eq!(report.sha256, sha256_file(&bundle).unwrap());
    // The same install always gives the same bundle (so a recorded hash can be reproduced).
    let again = a.path().join("again.hsmod");
    assert_eq!(export_of(&src, &m, &again).sha256, report.sha256);

    let result = go(&dst, &bundle, &official(&m, &report.sha256), &m, &ok, &ok).unwrap();
    assert_eq!(result.installed_components.len(), 3);
    assert!(result.reused_components.is_empty());
    let moved = |l: &Layout| {
        tree(&l.base)
            .into_iter()
            .filter(|f| !f.contains("pyvenv.cfg"))
            .map(|f| f.replace(&l.base.display().to_string(), "<base>"))
            .collect::<Vec<_>>()
    };
    assert_eq!(moved(&src), moved(&dst));
    // The venv config points at the new data directory, not the old one.
    let cfg = std::fs::read_to_string(dst.pyroot().join("envs").join(ENV).join("venv/pyvenv.cfg"))
        .unwrap();
    assert!(
        cfg.contains(&dst.pyroot().join("python").display().to_string()),
        "{cfg}"
    );
    assert!(!cfg.contains(&src.base.display().to_string()), "{cfg}");
    let data = dst.load().unwrap();
    assert_eq!(data.modules["speech"].status, Status::Installed);
    assert_eq!(
        data.modules["speech"].installed_version.as_deref(),
        Some("0.1.0")
    );
    assert!(data.transaction.is_none());
    for c in &m.components {
        assert_eq!(
            data.components[&c.id].owners,
            BTreeSet::from(["speech".to_string()])
        );
    }
    // No staging folder or backup stays behind.
    let left: Vec<_> = std::fs::read_dir(&dst.base)
        .unwrap()
        .flatten()
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .collect();
    assert_eq!(left, ["pyenv"]);
    // Installing the same version again is refused, nothing changes.
    let before = tree(&dst.base);
    assert_eq!(
        go(&dst, &bundle, &official(&m, &report.sha256), &m, &ok, &ok).unwrap_err(),
        "modules.already_installed"
    );
    assert_eq!(tree(&dst.base), before);
}

#[test]
fn tampered_unknown_and_mismatching_bundles_are_refused_before_anything_is_written() {
    let m = manifest();
    let (a, b) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let (src, dst) = (layout(a.path()), layout(b.path()));
    install_fixture(&src, &m, "one");
    let bundle = a.path().join("speech.hsmod");
    let report = export_of(&src, &m, &bundle);

    // No index entry for this file: unofficial. The shipped index never admits a test bundle.
    assert_eq!(
        go(&dst, &bundle, &[], &m, &ok, &ok).unwrap_err(),
        "modules.bundle_unofficial"
    );
    assert_eq!(
        go(&dst, &bundle, &official_index(), &m, &ok, &ok).unwrap_err(),
        "modules.bundle_unofficial"
    );
    // A changed file no longer has the recorded hash.
    let tampered = a.path().join("tampered.hsmod");
    rewrite(&bundle, &tampered, &|name, mut bytes| {
        if name.ends_with("uv.bin") {
            bytes.push(b'!');
        }
        bytes
    });
    assert_eq!(
        go(&dst, &tampered, &official(&m, &report.sha256), &m, &ok, &ok).unwrap_err(),
        "modules.bundle_unofficial"
    );
    // Even when the whole file is (wrongly) recorded, every file is checked against the manifest.
    let corrupt_sha = rewrite(&bundle, &tampered, &|name, mut bytes| {
        if name.ends_with("uv.bin") {
            bytes[0] ^= 1;
        }
        bytes
    });
    assert_eq!(
        go(&dst, &tampered, &official(&m, &corrupt_sha), &m, &ok, &ok).unwrap_err(),
        "modules.bundle_corrupt"
    );
    // An extra file the manifest does not list.
    let extra = a.path().join("extra.hsmod");
    rewrite(&bundle, &extra, &|_, b| b);
    {
        let file = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .open(&extra)
            .unwrap();
        let mut w = zip::ZipWriter::new_append(file).unwrap();
        w.start_file(
            "payload/pyenv/evil",
            zip::write::SimpleFileOptions::default(),
        )
        .unwrap();
        w.write_all(b"x").unwrap();
        w.finish().unwrap();
    }
    let extra_sha = sha256_file(&extra).unwrap();
    assert_eq!(
        go(&dst, &extra, &official(&m, &extra_sha), &m, &ok, &ok).unwrap_err(),
        "modules.bundle_corrupt"
    );
    // Not an archive at all.
    let junk = a.path().join("junk.hsmod");
    std::fs::write(&junk, b"not a zip").unwrap();
    let junk_sha = sha256_file(&junk).unwrap();
    assert_eq!(
        go(&dst, &junk, &official(&m, &junk_sha), &m, &ok, &ok).unwrap_err(),
        "modules.bundle_corrupt"
    );

    // Right file, wrong identity in the index: another module id or another version.
    for (id, version) in [("agents", m.version.as_str()), ("speech", "9.0.0")] {
        let index = vec![OfficialBundle {
            id: id.into(),
            version: version.into(),
            sha256: report.sha256.clone(),
        }];
        assert_eq!(
            go(&dst, &bundle, &index, &m, &ok, &ok).unwrap_err(),
            "modules.bundle_mismatch"
        );
    }
    // The app must install exactly this version of the module.
    let mut newer = m.clone();
    newer.version = "0.2.0".into();
    assert_eq!(
        go(
            &dst,
            &bundle,
            &official(&m, &report.sha256),
            &newer,
            &ok,
            &ok
        )
        .unwrap_err(),
        "modules.bundle_mismatch"
    );
    // Another platform.
    let mut other = m.clone();
    other.platforms = vec!["plan9-m68k".into()];
    assert!(go(
        &dst,
        &bundle,
        &official(&m, &report.sha256),
        &other,
        &ok,
        &ok
    )
    .is_err());
    // A manifest that wants to write outside its component's folders.
    let evil = a.path().join("evil.hsmod");
    let sha = rewrite(&bundle, &evil, &|name, bytes| {
        if name == MANIFEST {
            String::from_utf8(bytes)
                .unwrap()
                .replace("pyenv/envs/transcribe-cpp", "pyenv/envs/../../evil")
                .into_bytes()
        } else {
            bytes
        }
    });
    assert_eq!(
        go(&dst, &evil, &official(&m, &sha), &m, &ok, &ok).unwrap_err(),
        "modules.bundle_mismatch"
    );
    // Nothing of any of that reached the destination.
    assert!(!dst.store_file.exists());
    assert!(tree(&dst.base).is_empty(), "{:?}", tree(&dst.base));
    assert!(!b.path().join("evil").exists());
}

#[test]
fn a_failure_after_the_first_component_moved_restores_everything() {
    let m = manifest();
    let (a, b) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let (src, dst) = (layout(a.path()), layout(b.path()));
    install_fixture(&src, &m, "new");
    let bundle = a.path().join("speech.hsmod");
    let index = official(&m, &export_of(&src, &m, &bundle).sha256);

    // Fresh machine: failure in the middle leaves nothing and no store entry.
    let moved = StdMutex::new(Vec::new());
    let stop = |id: &str| {
        let mut moved = moved.lock().unwrap();
        moved.push(id.to_string());
        if moved.len() == 2 {
            Err("boom".into())
        } else {
            Ok(())
        }
    };
    assert_eq!(
        go(&dst, &bundle, &index, &m, &ok, &stop).unwrap_err(),
        "boom"
    );
    assert_eq!(moved.lock().unwrap().len(), 2);
    assert!(tree(&dst.base).is_empty(), "{:?}", tree(&dst.base));
    assert_eq!(dst.load().unwrap(), ModuleStore::default());

    // A module that is already installed (older version) is put back byte for byte.
    let mut old = m.clone();
    old.version = "0.0.9".into();
    install_fixture(&dst, &old, "old");
    let (files, store) = (tree(&dst.base), dst.load().unwrap());
    let failing_verify = |_: &str| Err("modules.interrupted".to_string());
    assert_eq!(
        go(&dst, &bundle, &index, &m, &failing_verify, &ok).unwrap_err(),
        "modules.interrupted"
    );
    assert_eq!(tree(&dst.base), files);
    assert_eq!(dst.load().unwrap(), store);
    // The retry that works replaces it and drops the backup.
    go(&dst, &bundle, &index, &m, &ok, &ok).unwrap();
    assert_eq!(
        dst.load().unwrap().modules["speech"]
            .installed_version
            .as_deref(),
        Some("0.1.0")
    );
    assert!(tree(&dst.base).iter().any(|f| f.contains("# new")));
    assert!(!tree(&dst.base).iter().any(|f| f.contains("# old")));
    assert!(!dst.pyroot().join("envs/transcribe-cpp.rollback").exists());
}

#[test]
fn shared_components_that_are_already_installed_are_reused_not_duplicated() {
    let m = manifest();
    let (a, b) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let (src, dst) = (layout(a.path()), layout(b.path()));
    install_fixture(&src, &m, "bundle");
    let bundle = a.path().join("speech.hsmod");
    let index = official(&m, &export_of(&src, &m, &bundle).sha256);

    // Another module on the destination already owns the shared uv and Python.
    let mut other = m.clone();
    other.id = "other".into();
    install_fixture(&dst, &other, "local");
    std::fs::remove_dir_all(dst.pyroot().join("envs")).unwrap();
    let mut data = dst.load().unwrap();
    data.components.remove(ENV);
    dst.store().save(&data).unwrap();

    // A failed import leaves the owners exactly as they were.
    let failing = |_: &str| Err("boom".to_string());
    assert!(go(&dst, &bundle, &index, &m, &failing, &ok).is_err());
    let data = dst.load().unwrap();
    assert_eq!(
        data.components["managed-uv"].owners,
        BTreeSet::from(["other".to_string()])
    );
    assert!(data
        .modules
        .get("speech")
        .is_none_or(|s| s.status == Status::NotInstalled));

    let report = go(&dst, &bundle, &index, &m, &ok, &ok).unwrap();
    assert_eq!(report.installed_components, [ENV]);
    assert_eq!(report.reused_components, ["managed-uv", "managed-python"]);
    let data = dst.load().unwrap();
    for shared in ["managed-uv", "managed-python"] {
        assert_eq!(
            data.components[shared].owners,
            BTreeSet::from(["other".to_string(), "speech".to_string()])
        );
    }
    // The existing copy stays as it was; the bundle's copy was not written beside it.
    let py = dst.pyroot();
    assert_eq!(
        std::fs::read_to_string(py.join("uv").join(pyenv::UV_VERSION).join("uv.bin")).unwrap(),
        "uv local"
    );
    assert_eq!(std::fs::read_dir(py.join("python")).unwrap().count(), 1);
    // Removing the module keeps what the other module still owns.
    let mut data = data;
    assert_eq!(data.release("speech"), [ENV]);
    assert_eq!(
        data.components["managed-uv"].owners,
        BTreeSet::from(["other".to_string()])
    );
}

#[test]
fn a_shared_component_with_another_version_in_use_blocks_the_import() {
    let m = manifest();
    let (a, b) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let (src, dst) = (layout(a.path()), layout(b.path()));
    install_fixture(&src, &m, "bundle");
    let bundle = a.path().join("speech.hsmod");
    let index = official(&m, &export_of(&src, &m, &bundle).sha256);
    let mut other = m.clone();
    other.id = "other".into();
    install_fixture(&dst, &other, "local");
    let mut data = dst.load().unwrap();
    data.components.get_mut("managed-python").unwrap().version = "old".into();
    dst.store().save(&data).unwrap();
    let before = (tree(&dst.base), dst.load().unwrap());
    assert_eq!(
        go(&dst, &bundle, &index, &m, &ok, &ok).unwrap_err(),
        "modules.shared_conflict"
    );
    assert_eq!((tree(&dst.base), dst.load().unwrap()), before);
}

#[test]
fn export_needs_an_installed_managed_module() {
    let m = manifest();
    let a = tempfile::tempdir().unwrap();
    let l = layout(a.path());
    let out = a.path().join("x.hsmod");
    let export =
        |l: &Layout, m: &ModuleManifest| export_at(l, m, &l.load().unwrap(), &host(), APP, &out);
    assert_eq!(export(&l, &m).unwrap_err(), "modules.not_installed");
    install_fixture(&l, &m, "one");
    let mut planned = m.clone();
    planned.planned = true;
    assert_eq!(export(&l, &planned).unwrap_err(), "modules.planned");
    let mut data = l.load().unwrap();
    data.components.get_mut(ENV).unwrap().version = "external:1".into();
    assert_eq!(
        export_at(&l, &m, &data, &host(), APP, &out).unwrap_err(),
        "modules.export_external"
    );
    std::fs::remove_dir_all(l.pyroot().join("uv")).unwrap();
    assert_eq!(export(&l, &m).unwrap_err(), "modules.export_missing");
    assert!(!out.exists() && !a.path().join("x.hsmod.part").exists());
}

#[test]
fn links_stay_inside_the_data_directory() {
    let new = Path::new("/new/bin");
    assert_eq!(
        relocate_link(
            "/old/bin",
            new,
            "pyenv/envs/e/venv/bin/python",
            "/old/bin/pyenv/python/cpython/bin/python3"
        ),
        Some(rel_to_path(new, "pyenv/python/cpython/bin/python3"))
    );
    assert!(relocate_link("/old/bin", new, "a/b", "/usr/bin/python3").is_none());
    assert!(relocate_link("/old/bin", new, "a/b", "/old/bin/../etc/passwd").is_none());
    assert!(relocate_link("/old/bin", new, "a/b", "python3").is_some());
    assert!(relocate_link("/old/bin", new, "a/b", "../../../etc").is_none());
    assert!(relocate_link("C:\\old", new, "a/b", "C:\\elsewhere").is_none());
}

#[test]
fn a_bundle_may_only_use_the_known_folders_of_its_components() {
    let c = |id: &str, version: &str| BundleComponent {
        id: id.into(),
        version: version.into(),
        roots: vec![],
        files: vec![],
        links: vec![],
    };
    assert!(root_allowed(&c(ENV, "x"), "pyenv/envs/transcribe-cpp"));
    assert!(!root_allowed(&c(ENV, "x"), "pyenv/envs/other"));
    assert!(root_allowed(&c("hermes-agent", "x"), "modules/hermes"));
    assert!(root_allowed(&c("managed-uv", "1.2.3"), "pyenv/uv/1.2.3"));
    assert!(!root_allowed(&c("managed-uv", "1.2.3"), "pyenv/uv/1.2.4"));
    let python = format!("pyenv/python/cpython-{}.1-x", pyenv::PYTHON_SERIES);
    assert!(root_allowed(&c("managed-python", "3.12"), &python));
    assert!(!root_allowed(
        &c("managed-python", "3.12"),
        &format!("{python}/..")
    ));
    assert!(!root_allowed(&c("managed-python", "3.12"), "pyenv/python"));
    assert!(!root_allowed(&c("anything", "x"), "modules/hermes"));
    for bad in ["", "/abs", "a//b", "a/../b", "a\\b", "c:/x", "./a"] {
        assert!(!valid_rel(bad), "{bad:?}");
    }
}

#[test]
fn official_index_lists_only_well_formed_released_bundles() {
    let known = bundled();
    let mut seen = HashSet::new();
    for entry in official_index() {
        let module = known
            .iter()
            .find(|m| m.id == entry.id)
            .expect("known module");
        assert!(!module.planned);
        assert_eq!(
            entry.version, module.version,
            "an old bundle cannot stay in the index"
        );
        assert!(
            entry.sha256.len() == 64
                && entry
                    .sha256
                    .chars()
                    .all(|c| matches!(c, '0'..='9' | 'a'..='f'))
        );
        assert!(seen.insert(entry.sha256));
    }
}

#[test]
fn the_layout_matches_where_the_installers_put_things() {
    let l = Layout::global();
    assert_eq!(l.pyroot(), envs::root());
    assert_eq!(l.store_file, paths::config_dir().join("modules.json"));
    assert_eq!(
        rel_to_path(&l.base, "pyenv/envs/transcribe-cpp"),
        lifecycle::runtime("speech")
    );
    assert_eq!(
        rel_to_path(&l.base, "modules/hermes"),
        lifecycle::runtime("agents")
    );
    assert_eq!(
        rel_to_path(&l.base, "pyenv/envs/transcribe-cpp").with_extension("rollback"),
        lifecycle::backup("speech")
    );
}

/// Removal is a pure filesystem and state change. This runs it on the process's isolated data
/// root, then installs the bundle back with no module code reaching the network.
#[test]
fn removal_and_reinstall_from_a_bundle_work_without_a_network() {
    let source = include_str!("../lifecycle.rs");
    let start = source.find("pub fn remove_with_options").unwrap();
    let body = &source[start..start + source[start..].find("#[cfg(test)]").unwrap()];
    assert!(
        !body.contains("reqwest") && !body.contains(".await"),
        "removal must stay offline"
    );

    let m = bundled().remove(0);
    if installable("speech").is_err() {
        return; // This host cannot run the speech backend, so the module cannot be installed.
    }
    let l = Layout::global();
    let tmp = tempfile::tempdir().unwrap();
    populate(&l, "global");
    let mut data = ModuleStore::default();
    data.claim(&m);
    store().save(&data).unwrap();

    let out = tmp.path().join("speech.hsmod");
    let report = export("speech", &out).unwrap();
    remove("speech").unwrap();
    assert!(!lifecycle::runtime("speech").exists());
    assert_eq!(
        load().unwrap().modules["speech"].status,
        Status::NotInstalled
    );
    // Removal reclaims shared uv and Python when nothing else needs them; make sure both are
    // gone either way, to install everything again.
    for shared in ["uv", "python"] {
        let _ = std::fs::remove_dir_all(l.pyroot().join(shared));
    }

    let resolve = |id: &str| installable(id);
    let before = |_: &str, _: bool| Ok(());
    let result = import_at(
        &l,
        &out,
        &official(&m, &report.sha256),
        &host(),
        env!("CARGO_PKG_VERSION"),
        &Hooks {
            resolve: &resolve,
            before_commit: &before,
            verify: &ok,
            after_move: &ok,
        },
        &STATE_LOCK,
    )
    .unwrap();
    assert_eq!(result.installed_components.len(), 3);
    assert_eq!(load().unwrap().modules["speech"].status, Status::Installed);
    assert_eq!(
        std::fs::read_to_string(lifecycle::runtime("speech").join("venv/lib/pkg/__init__.py"))
            .unwrap(),
        "# global"
    );
    remove("speech").unwrap();
    assert_eq!(
        load().unwrap().modules["speech"].status,
        Status::NotInstalled
    );

    let _ = std::fs::remove_dir_all(l.pyroot().join("uv"));
    let _ = std::fs::remove_dir_all(l.pyroot().join("python"));
    let _ = std::fs::remove_file(&l.store_file);
}
