use crate::config::AppSettings;
use crate::error_key::err;
use crate::models::LocalModel;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};
use sysinfo::Disks;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StorageDirInfo {
    pub path: String,
    pub is_primary: bool,
    pub exists: bool,
    pub free_bytes: u64,
    pub total_bytes: u64,
    pub model_count: usize,
}

pub struct StorageManager;

/// Windows `canonicalize` returns verbatim paths (`\\?\C:\...`) whose prefix never matches a
/// disk mount point (`C:\`), so free space was reported as 0 B. Strips the verbatim marker
/// (`\\?\UNC\server\share` becomes `\\server\share`).
fn without_verbatim_prefix(path: PathBuf) -> PathBuf {
    // A path that is not valid Unicode is left as it was rather than rebuilt lossily.
    let Some(text) = path.to_str() else {
        return path;
    };
    if let Some(rest) = text.strip_prefix(r"\\?\UNC\") {
        return PathBuf::from(format!(r"\\{rest}"));
    }
    match text.strip_prefix(r"\\?\") {
        Some(rest) => PathBuf::from(rest),
        None => path,
    }
}

impl StorageManager {
    /// Queries disk available and total space for the given directory path
    pub fn get_directory_disk_space(dir: &Path) -> (u64, u64) {
        let disks = Disks::new_with_refreshed_list();
        let canonical =
            without_verbatim_prefix(fs::canonicalize(dir).unwrap_or_else(|_| dir.to_path_buf()));
        let mut best_match: Option<(u64, u64, usize)> = None;

        for disk in &disks {
            let mount = disk.mount_point();
            if canonical.strip_prefix(mount).is_ok() {
                let len = mount.as_os_str().len();
                if best_match.as_ref().is_none_or(|m| len > m.2) {
                    best_match = Some((disk.available_space(), disk.total_space(), len));
                }
            }
        }

        best_match.map(|m| (m.0, m.1)).unwrap_or((0, 0))
    }

    /// Lists all configured storage directories (primary + extra) with disk stats
    pub fn get_storage_directories(
        settings: &AppSettings,
        models: &[LocalModel],
    ) -> Vec<StorageDirInfo> {
        let mut results = Vec::new();

        // 1. Primary models directory
        let primary_path = PathBuf::from(&settings.models_dir);
        let exists = primary_path.exists();
        let (free, total) = Self::get_directory_disk_space(&primary_path);
        let count = models
            .iter()
            .filter(|m| m.root_dir == settings.models_dir)
            .count();

        results.push(StorageDirInfo {
            path: settings.models_dir.clone(),
            is_primary: true,
            exists,
            free_bytes: free,
            total_bytes: total,
            model_count: count,
        });

        // 2. Extra model directories
        for extra in &settings.extra_models_dirs {
            if extra == &settings.models_dir {
                continue;
            }
            let p = PathBuf::from(extra);
            let exists = p.exists();
            let (free, total) = Self::get_directory_disk_space(&p);
            let count = models.iter().filter(|m| m.root_dir == *extra).count();

            results.push(StorageDirInfo {
                path: extra.clone(),
                is_primary: false,
                exists,
                free_bytes: free,
                total_bytes: total,
                model_count: count,
            });
        }

        results
    }

    /// Adds a new directory to the library
    pub fn add_directory_to_settings(settings: &mut AppSettings, dir_path: &str) {
        if settings.models_dir != dir_path
            && !settings.extra_models_dirs.iter().any(|d| d == dir_path)
        {
            settings.extra_models_dirs.push(dir_path.to_string());
        }
    }

    /// Removes an extra directory from the library
    pub fn remove_directory_from_settings(
        settings: &mut AppSettings,
        dir_path: &str,
    ) -> Result<(), String> {
        if settings.models_dir == dir_path {
            return Err(err("storage.cannot_remove_primary").into());
        }
        settings.extra_models_dirs.retain(|d| d != dir_path);
        Ok(())
    }

    /// Sets the primary models directory on settings object
    pub fn set_primary_on_settings(settings: &mut AppSettings, new_primary: &str) {
        let old_primary = settings.models_dir.clone();
        if old_primary != new_primary && !settings.extra_models_dirs.contains(&old_primary) {
            settings.extra_models_dirs.push(old_primary);
        }
        settings.extra_models_dirs.retain(|d| d != new_primary);
        settings.models_dir = new_primary.to_string();
    }

    /// Adds a new directory to the library and saves
    pub fn add_storage_directory(dir_path: &str) -> Result<AppSettings, String> {
        let path = crate::paths::resolve_dir(dir_path.trim());
        let dir_path_owned = path.to_string_lossy().to_string();
        let dir_path = dir_path_owned.as_str();
        if !path.exists() {
            fs::create_dir_all(&path).map_err(|e| {
                String::from(
                    err("fs.create_dir_failed")
                        .with("path", path.display())
                        .with("error", e),
                )
            })?;
        }

        let mut settings = AppSettings::load();
        Self::add_directory_to_settings(&mut settings, dir_path);
        settings.save()?;
        Ok(settings)
    }

    /// Removes an extra directory from the library and saves
    pub fn remove_storage_directory(dir_path: &str) -> Result<AppSettings, String> {
        let mut settings = AppSettings::load();
        Self::remove_directory_from_settings(&mut settings, dir_path)?;
        settings.save()?;
        Ok(settings)
    }

    /// Sets the primary models directory and saves
    pub fn set_primary_storage_directory(new_primary: &str) -> Result<AppSettings, String> {
        let path = crate::paths::resolve_dir(new_primary.trim());
        let new_primary_owned = path.to_string_lossy().to_string();
        let new_primary = new_primary_owned.as_str();
        if !path.exists() {
            fs::create_dir_all(&path).map_err(|e| {
                String::from(
                    err("fs.create_dir_failed")
                        .with("path", path.display())
                        .with("error", e),
                )
            })?;
        }

        let mut settings = AppSettings::load();
        Self::set_primary_on_settings(&mut settings, new_primary);
        settings.save()?;
        Ok(settings)
    }

    /// Safely migrates all files of a model (including all split shards) to a target directory.
    /// Operates atomically with full rollback if any error occurs:
    /// 1. Verifies destination free space.
    /// 2. Copies files to target with `.migrating` suffix.
    /// 3. Verifies file sizes match source exactly.
    /// 4. Renames `.migrating` to final filename.
    /// 5. Safely deletes old source files only after all shards are completely verified.
    pub fn migrate_model(model: &LocalModel, target_dir_str: &str) -> Result<PathBuf, String> {
        if model.source.is_some() {
            return Err(err("storage.linked_not_movable").into());
        }
        let target_dir = PathBuf::from(target_dir_str);
        if !target_dir.exists() {
            fs::create_dir_all(&target_dir).map_err(|e| {
                String::from(
                    err("fs.create_dir_failed")
                        .with("path", target_dir.display())
                        .with("error", e),
                )
            })?;
        }

        // Check if model is already inside target_dir
        if model.root_dir == target_dir_str {
            return Err(err("storage.already_in_target").into());
        }

        // Disk space check: required = model.size_bytes + 50 MB buffer
        let (free_bytes, _) = Self::get_directory_disk_space(&target_dir);
        let required_bytes = model.size_bytes + (50 * 1024 * 1024);
        if free_bytes > 0 && free_bytes < required_bytes {
            return Err(err("storage.insufficient_space")
                .with(
                    "gb",
                    format!("{:.2}", required_bytes as f64 / (1024.0 * 1024.0 * 1024.0)),
                )
                .into());
        }

        let mut copied_temp_files: Vec<PathBuf> = Vec::new();
        let mut final_dest_files: Vec<(PathBuf, PathBuf)> = Vec::new(); // (temp_path, final_dest_path)

        // Step 1: Copy each shard with .migrating extension
        for shard_src in &model.shard_paths {
            if !shard_src.exists() {
                // Rollback
                Self::cleanup_files(&copied_temp_files);
                return Err(err("fs.not_found").with("path", shard_src.display()).into());
            }

            let file_name = match shard_src.file_name() {
                Some(n) => n,
                None => {
                    Self::cleanup_files(&copied_temp_files);
                    return Err(err("storage.invalid_file_name")
                        .with("path", shard_src.display())
                        .into());
                }
            };

            // Keep the model's sub-folder layout (e.g. <repo>/<file>.gguf) relative to its library root.
            let rel = shard_src
                .strip_prefix(&model.root_dir)
                .map(|r| r.to_path_buf())
                .unwrap_or_else(|_| PathBuf::from(file_name));
            let final_dest = target_dir.join(&rel);
            if final_dest.exists() {
                Self::cleanup_files(&copied_temp_files);
                return Err(err("storage.target_exists")
                    .with("path", final_dest.display())
                    .into());
            }
            if let Some(parent) = final_dest.parent() {
                if let Err(e) = fs::create_dir_all(parent) {
                    Self::cleanup_files(&copied_temp_files);
                    return Err(err("fs.create_dir_failed")
                        .with("path", parent.display())
                        .with("error", e)
                        .into());
                }
            }
            let temp_dest =
                final_dest.with_file_name(format!("{}.migrating", file_name.to_string_lossy()));

            // Copy file
            if let Err(e) = fs::copy(shard_src, &temp_dest) {
                Self::cleanup_files(&copied_temp_files);
                return Err(err("fs.copy_failed")
                    .with("from", shard_src.display())
                    .with("to", temp_dest.display())
                    .with("error", e)
                    .into());
            }
            copied_temp_files.push(temp_dest.clone());

            // Verify size
            let src_size = fs::metadata(shard_src).map(|m| m.len()).unwrap_or(0);
            let dest_size = fs::metadata(&temp_dest).map(|m| m.len()).unwrap_or(0);
            if src_size != dest_size {
                Self::cleanup_files(&copied_temp_files);
                return Err(err("storage.size_mismatch")
                    .with("path", temp_dest.display())
                    .into());
            }

            final_dest_files.push((temp_dest, final_dest));
        }

        // Step 2: Atomic rename of .migrating to final filenames
        let mut successfully_renamed: Vec<PathBuf> = Vec::new();
        for (temp_path, final_path) in &final_dest_files {
            if let Err(e) = fs::rename(temp_path, final_path) {
                // Rollback both temporary and renamed final files
                Self::cleanup_files(&copied_temp_files);
                Self::cleanup_files(&successfully_renamed);
                return Err(err("fs.rename_failed")
                    .with("from", temp_path.display())
                    .with("to", final_path.display())
                    .with("error", e)
                    .into());
            }
            successfully_renamed.push(final_path.clone());
        }

        // Step 3: Now that all destination files are 100% verified, safely delete old source files
        for shard_src in &model.shard_paths {
            let _ = fs::remove_file(shard_src);
            // Optionally clean up empty parent directories up to root_dir
            if let Some(parent) = shard_src.parent() {
                if parent != Path::new(&model.root_dir) && parent.exists() {
                    let is_empty = fs::read_dir(parent)
                        .map(|mut d| d.next().is_none())
                        .unwrap_or(false);
                    if is_empty {
                        let _ = fs::remove_dir(parent);
                    }
                }
            }
        }

        // Return new launch path (shard 1)
        let rel = model
            .path
            .strip_prefix(&model.root_dir)
            .map(|r| r.to_path_buf())
            .unwrap_or_else(|_| PathBuf::from(model.path.file_name().unwrap_or_default()));
        Ok(target_dir.join(rel))
    }

    fn cleanup_files(files: &[PathBuf]) {
        for file in files {
            if file.exists() {
                let _ = fs::remove_file(file);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn verbatim_prefix_is_removed_for_disk_matching() {
        assert_eq!(
            without_verbatim_prefix(PathBuf::from(r"\\?\C:\Users\a")),
            PathBuf::from(r"C:\Users\a")
        );
        assert_eq!(
            without_verbatim_prefix(PathBuf::from(r"\\?\UNC\srv\share\m")),
            PathBuf::from(r"\\srv\share\m")
        );
        assert_eq!(
            without_verbatim_prefix(PathBuf::from("/data/models")),
            PathBuf::from("/data/models")
        );
    }

    #[test]
    fn existing_directory_reports_disk_space() {
        let tmp = tempfile::tempdir().unwrap();
        let (free, total) = StorageManager::get_directory_disk_space(tmp.path());
        assert!(
            total > 0,
            "total space must not be 0 for an existing directory"
        );
        assert!(free > 0);
    }

    #[test]
    fn test_storage_add_and_remove_directory() {
        let tmp = tempfile::tempdir().unwrap();
        let temp_dir_buf = tmp.path().join("models_extra");
        let temp_dir = temp_dir_buf.to_str().unwrap();
        let mut s = AppSettings::default();

        StorageManager::add_directory_to_settings(&mut s, temp_dir);
        assert!(s.extra_models_dirs.contains(&temp_dir.to_string()));

        StorageManager::remove_directory_from_settings(&mut s, temp_dir).unwrap();
        assert!(!s.extra_models_dirs.contains(&temp_dir.to_string()));
    }

    #[test]
    fn test_storage_set_primary_preserves_old() {
        let tmp = tempfile::tempdir().unwrap();
        let old_buf = tmp.path().join("primary_old");
        let new_buf = tmp.path().join("primary_new");
        let old_primary = old_buf.to_str().unwrap();
        let new_primary = new_buf.to_str().unwrap();

        let mut s = AppSettings {
            models_dir: old_primary.to_string(),
            ..Default::default()
        };

        StorageManager::set_primary_on_settings(&mut s, new_primary);
        assert_eq!(s.models_dir, new_primary);
        assert!(s.extra_models_dirs.contains(&old_primary.to_string()));
    }

    #[test]
    fn test_safe_model_migration_single_and_split() {
        let tmp = tempfile::tempdir().unwrap();
        let src_dir = tmp.path().join("test_src_models");
        let dest_dir = tmp.path().join("test_dest_models");
        let _ = fs::create_dir_all(&src_dir);
        let _ = fs::create_dir_all(&dest_dir);

        // 1. Create a dummy split model with 2 shards
        let shard1 = src_dir.join("test-model-00001-of-00002.gguf");
        let shard2 = src_dir.join("test-model-00002-of-00002.gguf");
        fs::write(&shard1, b"GGUF_SHARD_1_CONTENT_DUMMY_BYTES_12345").unwrap();
        fs::write(&shard2, b"GGUF_SHARD_2_CONTENT_DUMMY_BYTES_67890").unwrap();

        let model = LocalModel {
            id: shard1.to_string_lossy().to_string(),
            name: "test-model".to_string(),
            filename: "test-model-00001-of-00002.gguf".to_string(),
            path: shard1.clone(),
            size_bytes: 38 + 38,
            architecture: Some("llama".to_string()),
            context_length: Some(4096),
            quantization: "Q4_K_M".to_string(),
            is_split: true,
            total_shards: Some(2),
            present_shards: Some(2),
            is_ready: true,
            shard_paths: vec![shard1.clone(), shard2.clone()],
            root_dir: src_dir.to_string_lossy().to_string(),
            gguf: None,
            source: None,
            mmproj_path: None,
            kind: Default::default(),
            origin: Default::default(),
            task: None,
        };

        // 2. Migrate to dest_dir
        let new_path = StorageManager::migrate_model(&model, dest_dir.to_str().unwrap())
            .expect("Migration failed");

        // 3. Verify destination files exist and match content
        let dest_shard1 = dest_dir.join("test-model-00001-of-00002.gguf");
        let dest_shard2 = dest_dir.join("test-model-00002-of-00002.gguf");
        assert!(dest_shard1.exists(), "Dest shard 1 must exist");
        assert!(dest_shard2.exists(), "Dest shard 2 must exist");
        assert_eq!(
            fs::read(&dest_shard1).unwrap(),
            b"GGUF_SHARD_1_CONTENT_DUMMY_BYTES_12345"
        );
        assert_eq!(
            fs::read(&dest_shard2).unwrap(),
            b"GGUF_SHARD_2_CONTENT_DUMMY_BYTES_67890"
        );
        assert_eq!(new_path, dest_shard1);

        // 4. Verify old source files are deleted
        assert!(!shard1.exists(), "Old shard 1 must be deleted");
        assert!(!shard2.exists(), "Old shard 2 must be deleted");

        // Cleanup
        let _ = fs::remove_dir_all(&src_dir);
        let _ = fs::remove_dir_all(&dest_dir);
    }

    #[test]
    fn test_migration_refuses_to_overwrite_and_keeps_subfolders() {
        let root = tempfile::tempdir().unwrap();
        let src_root = root.path().join("src");
        let dest = root.path().join("dest");
        let repo_dir = src_root.join("org").join("repo");
        fs::create_dir_all(&repo_dir).unwrap();
        fs::create_dir_all(&dest).unwrap();
        let file = repo_dir.join("m.gguf");
        fs::write(&file, b"GGUF-data").unwrap();

        let model = LocalModel {
            id: file.to_string_lossy().to_string(),
            name: "m".into(),
            filename: "m.gguf".into(),
            path: file.clone(),
            size_bytes: 9,
            architecture: None,
            context_length: None,
            quantization: "Q4_0".into(),
            is_split: false,
            total_shards: None,
            present_shards: None,
            is_ready: true,
            shard_paths: vec![file.clone()],
            root_dir: src_root.to_string_lossy().to_string(),
            gguf: None,
            source: None,
            mmproj_path: None,
            kind: Default::default(),
            origin: Default::default(),
            task: None,
        };

        // Collision: an existing file at the destination must not be overwritten.
        let clash = dest.join("org").join("repo").join("m.gguf");
        fs::create_dir_all(clash.parent().unwrap()).unwrap();
        fs::write(&clash, b"other").unwrap();
        assert!(StorageManager::migrate_model(&model, dest.to_str().unwrap()).is_err());
        assert!(file.exists(), "source must be untouched on failure");
        assert_eq!(fs::read(&clash).unwrap(), b"other");

        fs::remove_file(&clash).unwrap();
        let new_path = StorageManager::migrate_model(&model, dest.to_str().unwrap()).unwrap();
        assert_eq!(new_path, clash);
        assert!(!file.exists());
    }
}
