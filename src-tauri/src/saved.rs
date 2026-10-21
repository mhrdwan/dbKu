//! Saved queries: named SQL kept per connection, in a JSON file next to
//! the connection list.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::error::{AppError, AppResult};
use crate::store::{read_json_list, write_json};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SavedQuery {
    #[serde(default)]
    pub id: String,
    /// The connection it belongs to; `None` shows it under every connection.
    #[serde(default)]
    pub conn_id: Option<String>,
    pub name: String,
    pub sql: String,
    #[serde(default)]
    pub database: Option<String>,
    #[serde(default)]
    pub schema: Option<String>,
    /// Unix milliseconds.
    #[serde(default)]
    pub updated_at: i64,
}

pub struct SavedQueryStore {
    path: PathBuf,
}

impl SavedQueryStore {
    pub fn new(path: PathBuf) -> Self {
        Self { path }
    }

    pub fn list(&self) -> AppResult<Vec<SavedQuery>> {
        let mut all: Vec<SavedQuery> = read_json_list(&self.path)?;
        all.sort_by_key(|q| q.name.to_lowercase());
        Ok(all)
    }

    /// Insert (empty id) or update a query; returns it with id and time set.
    pub fn save(&self, query: SavedQuery) -> AppResult<SavedQuery> {
        let name = query.name.trim();
        if name.is_empty() {
            return Err(AppError::Invalid("a saved query needs a name".into()));
        }
        let saved = SavedQuery {
            id: if query.id.is_empty() {
                uuid::Uuid::new_v4().to_string()
            } else {
                query.id.clone()
            },
            name: name.to_string(),
            updated_at: chrono::Utc::now().timestamp_millis(),
            ..query
        };
        let mut all: Vec<SavedQuery> = read_json_list(&self.path)?;
        all.retain(|q| q.id != saved.id);
        all.push(saved.clone());
        write_json(&self.path, &all)?;
        Ok(saved)
    }

    pub fn delete(&self, id: &str) -> AppResult<()> {
        let mut all: Vec<SavedQuery> = read_json_list(&self.path)?;
        all.retain(|q| q.id != id);
        write_json(&self.path, &all)
    }

    /// Queries of a deleted connection go with it.
    pub fn delete_for_connection(&self, conn_id: &str) -> AppResult<()> {
        let mut all: Vec<SavedQuery> = read_json_list(&self.path)?;
        all.retain(|q| q.conn_id.as_deref() != Some(conn_id));
        write_json(&self.path, &all)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn query(name: &str, conn: Option<&str>) -> SavedQuery {
        SavedQuery {
            id: String::new(),
            conn_id: conn.map(String::from),
            name: name.into(),
            sql: "SELECT 1".into(),
            database: None,
            schema: None,
            updated_at: 0,
        }
    }

    #[test]
    fn saves_updates_and_deletes() {
        let dir = tempfile::tempdir().unwrap();
        let store = SavedQueryStore::new(dir.path().join("q.json"));
        let a = store.save(query("  Daily report ", Some("c1"))).unwrap();
        assert!(!a.id.is_empty() && a.updated_at > 0);
        assert_eq!(a.name, "Daily report");
        let renamed = store
            .save(SavedQuery {
                name: "Weekly".into(),
                sql: "SELECT 2".into(),
                ..a.clone()
            })
            .unwrap();
        assert_eq!(renamed.id, a.id);
        store.save(query("alpha", Some("c2"))).unwrap();
        let names: Vec<String> = store.list().unwrap().into_iter().map(|q| q.name).collect();
        assert_eq!(
            names,
            ["alpha", "Weekly"],
            "sorted, case-insensitive, updated in place"
        );
        store.delete(&a.id).unwrap();
        store.delete_for_connection("c2").unwrap();
        assert!(store.list().unwrap().is_empty());
    }

    #[test]
    fn needs_a_name() {
        let dir = tempfile::tempdir().unwrap();
        let store = SavedQueryStore::new(dir.path().join("q.json"));
        assert!(store.save(query("  ", None)).is_err());
    }
}
