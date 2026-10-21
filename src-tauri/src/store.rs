//! Saved connections: profiles in a JSON file, passwords in the OS keychain.

use std::path::{Path, PathBuf};

use crate::error::{AppError, AppResult};
use crate::model::ConnectionProfile;

const KEYCHAIN_SERVICE: &str = "dev.dbku.app";

pub trait SecretStore: Send + Sync {
    fn get(&self, id: &str) -> AppResult<Option<String>>;
    fn set(&self, id: &str, secret: &str) -> AppResult<()>;
    fn delete(&self, id: &str) -> AppResult<()>;
}

pub struct Keychain;

impl Keychain {
    fn entry(id: &str) -> AppResult<keyring::Entry> {
        keyring::Entry::new(KEYCHAIN_SERVICE, id).map_err(|e| AppError::Secret(e.to_string()))
    }
}

impl SecretStore for Keychain {
    fn get(&self, id: &str) -> AppResult<Option<String>> {
        match Self::entry(id)?.get_password() {
            Ok(p) => Ok(Some(p)),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(e) => Err(AppError::Secret(e.to_string())),
        }
    }

    fn set(&self, id: &str, secret: &str) -> AppResult<()> {
        Self::entry(id)?
            .set_password(secret)
            .map_err(|e| AppError::Secret(e.to_string()))
    }

    fn delete(&self, id: &str) -> AppResult<()> {
        match Self::entry(id)?.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(e) => Err(AppError::Secret(e.to_string())),
        }
    }
}

pub struct ConnectionStore {
    path: PathBuf,
    secrets: Box<dyn SecretStore>,
}

impl ConnectionStore {
    pub fn new(path: PathBuf, secrets: Box<dyn SecretStore>) -> Self {
        Self { path, secrets }
    }

    pub fn list(&self) -> AppResult<Vec<ConnectionProfile>> {
        read_profiles(&self.path)
    }

    pub fn get(&self, id: &str) -> AppResult<ConnectionProfile> {
        self.list()?
            .into_iter()
            .find(|p| p.id == id)
            .ok_or_else(|| AppError::Invalid(format!("unknown connection {id}")))
    }

    /// Insert or replace a profile. `password: None` keeps the stored one;
    /// turning `save_password` off removes it from the keychain.
    pub fn save(
        &self,
        profile: ConnectionProfile,
        password: Option<&str>,
    ) -> AppResult<ConnectionProfile> {
        validate(&profile)?;
        if profile.save_password {
            if let Some(pw) = password {
                self.secrets.set(&profile.id, pw)?;
            }
        } else {
            self.secrets.delete(&profile.id)?;
        }
        let others = self.list()?.into_iter().filter(|p| p.id != profile.id);
        let mut all: Vec<ConnectionProfile> = others.collect();
        all.push(profile.clone());
        write_profiles(&self.path, &all)?;
        Ok(profile)
    }

    pub fn delete(&self, id: &str) -> AppResult<()> {
        let remaining: Vec<ConnectionProfile> =
            self.list()?.into_iter().filter(|p| p.id != id).collect();
        write_profiles(&self.path, &remaining)?;
        self.secrets.delete(id)
    }

    pub fn password(&self, id: &str) -> AppResult<Option<String>> {
        self.secrets.get(id)
    }
}

fn validate(p: &ConnectionProfile) -> AppResult<()> {
    if p.id.trim().is_empty() || p.name.trim().is_empty() {
        return Err(AppError::Invalid(
            "connection needs an id and a name".into(),
        ));
    }
    let needs_host = p.driver != crate::model::Driver::Sqlite;
    if needs_host && p.host.trim().is_empty() {
        return Err(AppError::Invalid("host is required".into()));
    }
    if !needs_host && p.database.trim().is_empty() {
        return Err(AppError::Invalid("SQLite needs a database file".into()));
    }
    Ok(())
}

fn read_profiles(path: &Path) -> AppResult<Vec<ConnectionProfile>> {
    read_json_list(path)
}

fn write_profiles(path: &Path, profiles: &[ConnectionProfile]) -> AppResult<()> {
    write_json(path, profiles)
}

/// A JSON array from `path`; a missing file is an empty list.
pub(crate) fn read_json_list<T: serde::de::DeserializeOwned>(path: &Path) -> AppResult<Vec<T>> {
    match std::fs::read_to_string(path) {
        Ok(text) => Ok(serde_json::from_str(&text)?),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Vec::new()),
        Err(e) => Err(e.into()),
    }
}

/// Write to a temp file and rename, so a crash never leaves half a file.
pub(crate) fn write_json<T: serde::Serialize + ?Sized>(path: &Path, value: &T) -> AppResult<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, serde_json::to_vec_pretty(value)?)?;
    std::fs::rename(&tmp, path)?;
    Ok(())
}

#[cfg(test)]
pub mod tests {
    use super::*;
    use crate::model::{Driver, SslMode};
    use std::collections::HashMap;
    use std::sync::Mutex;

    #[derive(Default)]
    pub struct MemorySecrets(pub Mutex<HashMap<String, String>>);

    impl SecretStore for MemorySecrets {
        fn get(&self, id: &str) -> AppResult<Option<String>> {
            Ok(self.0.lock().unwrap().get(id).cloned())
        }
        fn set(&self, id: &str, secret: &str) -> AppResult<()> {
            self.0.lock().unwrap().insert(id.into(), secret.into());
            Ok(())
        }
        fn delete(&self, id: &str) -> AppResult<()> {
            self.0.lock().unwrap().remove(id);
            Ok(())
        }
    }

    pub fn profile(id: &str, driver: Driver) -> ConnectionProfile {
        ConnectionProfile {
            id: id.into(),
            name: format!("conn {id}"),
            driver,
            host: "localhost".into(),
            port: driver.default_port(),
            user: "me".into(),
            database: "app".into(),
            ssl_mode: SslMode::Prefer,
            trust_server_cert: false,
            legacy_tls: false,
            read_only: false,
            color: None,
            group: None,
            save_password: true,
        }
    }

    fn store(dir: &tempfile::TempDir) -> ConnectionStore {
        ConnectionStore::new(
            dir.path().join("c.json"),
            Box::new(MemorySecrets::default()),
        )
    }

    #[test]
    fn saves_lists_and_replaces_profiles() {
        let dir = tempfile::tempdir().unwrap();
        let s = store(&dir);
        assert!(s.list().unwrap().is_empty());
        s.save(profile("a", Driver::Postgres), Some("pw")).unwrap();
        s.save(profile("b", Driver::Mysql), None).unwrap();
        let mut renamed = profile("a", Driver::Postgres);
        renamed.name = "renamed".into();
        s.save(renamed, None).unwrap();

        let all = s.list().unwrap();
        assert_eq!(all.len(), 2);
        assert_eq!(s.get("a").unwrap().name, "renamed");
        // Saving without a password keeps the old one.
        assert_eq!(s.password("a").unwrap().as_deref(), Some("pw"));
    }

    #[test]
    fn password_never_lands_in_the_json_file() {
        let dir = tempfile::tempdir().unwrap();
        let s = store(&dir);
        s.save(profile("a", Driver::Postgres), Some("hunter2-secret"))
            .unwrap();
        let text = std::fs::read_to_string(dir.path().join("c.json")).unwrap();
        assert!(
            text.contains("\"id\": \"a\""),
            "sanity: file has the profile"
        );
        assert!(!text.contains("hunter2-secret"));
    }

    #[test]
    fn turning_off_save_password_forgets_it() {
        let dir = tempfile::tempdir().unwrap();
        let s = store(&dir);
        s.save(profile("a", Driver::Postgres), Some("pw")).unwrap();
        let mut p = profile("a", Driver::Postgres);
        p.save_password = false;
        s.save(p, Some("ignored")).unwrap();
        assert_eq!(s.password("a").unwrap(), None);
    }

    #[test]
    fn delete_removes_profile_and_password() {
        let dir = tempfile::tempdir().unwrap();
        let s = store(&dir);
        s.save(profile("a", Driver::Postgres), Some("pw")).unwrap();
        s.delete("a").unwrap();
        assert!(s.list().unwrap().is_empty());
        assert_eq!(s.password("a").unwrap(), None);
    }

    #[test]
    fn rejects_invalid_profiles() {
        let dir = tempfile::tempdir().unwrap();
        let s = store(&dir);
        let mut no_host = profile("a", Driver::Postgres);
        no_host.host = " ".into();
        assert!(s.save(no_host, None).is_err());
        let mut sqlite = profile("b", Driver::Sqlite);
        sqlite.database = "".into();
        assert!(s.save(sqlite, None).is_err());
    }
}
