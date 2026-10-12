use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use tokio::sync::{OwnedRwLockReadGuard, OwnedRwLockWriteGuard, RwLock};

#[derive(Default)]
pub struct OperationGate {
    runtime: Arc<RwLock<()>>,
    models: Mutex<HashMap<PathBuf, Arc<RwLock<()>>>>,
}

impl OperationGate {
    pub async fn runtime_read(&self) -> OwnedRwLockReadGuard<()> {
        self.runtime.clone().read_owned().await
    }

    pub async fn runtime_write(&self) -> OwnedRwLockWriteGuard<()> {
        self.runtime.clone().write_owned().await
    }

    pub async fn model_read(&self, path: &Path) -> OwnedRwLockReadGuard<()> {
        self.model_lock(path).read_owned().await
    }

    pub async fn model_write(&self, path: &Path) -> OwnedRwLockWriteGuard<()> {
        self.model_lock(path).write_owned().await
    }

    fn model_lock(&self, path: &Path) -> Arc<RwLock<()>> {
        let key = normalized_path(path);
        self.models
            .lock()
            .unwrap()
            .entry(key)
            .or_insert_with(|| Arc::new(RwLock::new(())))
            .clone()
    }
}

pub fn normalized_path(path: &Path) -> PathBuf {
    let absolute = std::fs::canonicalize(path).unwrap_or_else(|_| {
        if path.is_absolute() {
            path.to_path_buf()
        } else {
            std::env::current_dir()
                .unwrap_or_else(|_| PathBuf::from("."))
                .join(path)
        }
    });
    #[cfg(windows)]
    {
        PathBuf::from(absolute.to_string_lossy().to_lowercase())
    }
    #[cfg(not(windows))]
    {
        absolute
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn writer_waits_for_model_reader() {
        let gate = Arc::new(OperationGate::default());
        let read = gate.model_read(Path::new("model.gguf")).await;
        let waiting = {
            let gate = gate.clone();
            tokio::spawn(async move { gate.model_write(Path::new("model.gguf")).await })
        };
        tokio::task::yield_now().await;
        assert!(!waiting.is_finished());
        drop(read);
        waiting.await.unwrap();
    }

    #[cfg(windows)]
    #[test]
    fn windows_keys_are_case_insensitive() {
        assert_eq!(
            normalized_path(Path::new("C:\\MODELS\\A.GGUF")),
            normalized_path(Path::new("c:\\models\\a.gguf"))
        );
    }
}
