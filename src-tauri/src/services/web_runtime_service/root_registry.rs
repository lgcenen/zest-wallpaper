use std::{
    collections::HashMap,
    path::PathBuf,
    sync::{Arc, Mutex as StdMutex},
};

#[derive(Clone)]
pub(super) struct RuntimeRootRegistry {
    roots: Arc<StdMutex<HashMap<String, PathBuf>>>,
}

impl RuntimeRootRegistry {
    pub(super) fn new() -> Self {
        Self {
            roots: Arc::new(StdMutex::new(HashMap::new())),
        }
    }

    pub(super) fn register(&self, token: String, root: PathBuf) -> Result<(), String> {
        let mut roots = self.roots.lock().map_err(|error| error.to_string())?;
        roots.insert(token, root);
        Ok(())
    }

    pub(super) fn resolve(&self, token: &str) -> Result<Option<PathBuf>, String> {
        let roots = self.roots.lock().map_err(|error| error.to_string())?;
        Ok(roots.get(token).cloned())
    }
}
