//! Where provider keys and sign-in tokens live: a private file in the app's config
//! folder (the app), or memory (tests).

use std::collections::{BTreeMap, HashMap};
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

pub trait Secrets: Send + Sync {
    fn get(&self, provider: &str) -> Result<Option<String>, String>;
    fn set(&self, provider: &str, key: &str) -> Result<(), String>;
    fn delete(&self, provider: &str) -> Result<(), String>;
}

/// A JSON file only the user can read (mode 600), in Doc Forge's own config folder.
/// No system keychain, so no permission prompts. Writes are atomic (temp file, then
/// rename), and the file is created private rather than tightened afterwards.
pub struct FileSecrets {
    path: PathBuf,
    cache: Mutex<Option<BTreeMap<String, String>>>,
}

impl FileSecrets {
    pub fn new(path: impl Into<PathBuf>) -> Self {
        FileSecrets {
            path: path.into(),
            cache: Mutex::new(None),
        }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    fn with<T>(&self, f: impl FnOnce(&mut BTreeMap<String, String>) -> T) -> T {
        let mut cache = self.cache.lock().unwrap();
        let map = cache.get_or_insert_with(|| {
            fs::read(&self.path)
                .ok()
                .and_then(|b| serde_json::from_slice(&b).ok())
                .unwrap_or_default()
        });
        f(map)
    }

    fn write(&self, map: &BTreeMap<String, String>) -> Result<(), String> {
        if let Some(dir) = self.path.parent() {
            fs::create_dir_all(dir).map_err(|e| e.to_string())?;
        }
        let tmp = self.path.with_extension("json.tmp");
        let bytes = serde_json::to_vec_pretty(map).map_err(|e| e.to_string())?;
        let mut opts = fs::OpenOptions::new();
        opts.write(true).create(true).truncate(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            opts.mode(0o600);
        }
        let mut f = opts.open(&tmp).map_err(|e| e.to_string())?;
        f.write_all(&bytes).map_err(|e| e.to_string())?;
        f.sync_all().map_err(|e| e.to_string())?;
        #[cfg(unix)]
        {
            // In case a temp file was left behind with other permissions.
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&tmp, fs::Permissions::from_mode(0o600))
                .map_err(|e| e.to_string())?;
        }
        fs::rename(&tmp, &self.path).map_err(|e| e.to_string())
    }
}

impl Secrets for FileSecrets {
    fn get(&self, provider: &str) -> Result<Option<String>, String> {
        Ok(self.with(|m| m.get(provider).cloned()))
    }

    fn set(&self, provider: &str, key: &str) -> Result<(), String> {
        self.with(|m| {
            let mut next = m.clone();
            next.insert(provider.to_string(), key.to_string());
            self.write(&next)?;
            *m = next;
            Ok(())
        })
    }

    fn delete(&self, provider: &str) -> Result<(), String> {
        self.with(|m| {
            if !m.contains_key(provider) {
                return Ok(());
            }
            let mut next = m.clone();
            next.remove(provider);
            self.write(&next)?;
            *m = next;
            Ok(())
        })
    }
}

#[derive(Default)]
pub struct MemorySecrets(Mutex<HashMap<String, String>>);

impl Secrets for MemorySecrets {
    fn get(&self, provider: &str) -> Result<Option<String>, String> {
        Ok(self.0.lock().unwrap().get(provider).cloned())
    }

    fn set(&self, provider: &str, key: &str) -> Result<(), String> {
        self.0
            .lock()
            .unwrap()
            .insert(provider.to_string(), key.to_string());
        Ok(())
    }

    fn delete(&self, provider: &str) -> Result<(), String> {
        self.0.lock().unwrap().remove(provider);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn file_secrets_persist_privately() {
        let dir = std::env::temp_dir().join(format!("forge-secrets-{}", std::process::id()));
        let path = dir.join("credentials.json");
        let s = FileSecrets::new(&path);
        assert_eq!(s.get("fireworks").unwrap(), None);
        s.set("fireworks", "fw_abc").unwrap();
        s.set("search-exa", "x").unwrap();
        s.delete("search-exa").unwrap();
        s.delete("missing").unwrap();
        // A fresh reader sees what was written.
        let again = FileSecrets::new(&path);
        assert_eq!(again.get("fireworks").unwrap().as_deref(), Some("fw_abc"));
        assert_eq!(again.get("search-exa").unwrap(), None);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = fs::metadata(&path).unwrap().permissions().mode() & 0o777;
            assert_eq!(mode, 0o600);
        }
        let _ = fs::remove_dir_all(dir);
    }
}
