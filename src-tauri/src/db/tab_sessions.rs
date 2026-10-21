//! Dedicated connections for query tabs.
//!
//! A query tab behaves like its own session, as in other database clients:
//! a `BEGIN`, a `SET` or a temp table stays with that tab. Pooled connections
//! can't give that guarantee (the next run, or a grid save, may land on a
//! different one), so each tab gets a connection of its own, kept here
//! between runs and closed when the tab closes.

use std::collections::HashMap;

use tokio::sync::Mutex;

pub struct TabSessions<C> {
    /// tab key → (database the connection is on, connection)
    map: Mutex<HashMap<String, (String, C)>>,
}

impl<C> Default for TabSessions<C> {
    fn default() -> Self {
        Self {
            map: Mutex::new(HashMap::new()),
        }
    }
}

impl<C> TabSessions<C> {
    /// Take the tab's connection out for a run. A connection to a different
    /// database is dropped (Postgres connections can't switch databases).
    pub async fn take(&self, key: &str, database: &str) -> Option<C> {
        let (db, conn) = self.map.lock().await.remove(key)?;
        (db == database).then_some(conn)
    }

    /// Hand the connection back after a successful run.
    pub async fn put(&self, key: &str, database: &str, conn: C) {
        self.map
            .lock()
            .await
            .insert(key.to_string(), (database.to_string(), conn));
    }

    pub async fn remove(&self, key: &str) -> Option<C> {
        self.map.lock().await.remove(key).map(|(_, c)| c)
    }

    pub async fn drain(&self) -> Vec<C> {
        self.map.lock().await.drain().map(|(_, (_, c))| c).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn keeps_a_connection_per_tab_and_database() {
        let s = TabSessions::<u32>::default();
        s.put("tab-1", "app", 7).await;
        assert_eq!(s.take("tab-1", "app").await, Some(7));
        assert_eq!(
            s.take("tab-1", "app").await,
            None,
            "taken out while running"
        );

        s.put("tab-1", "app", 7).await;
        assert_eq!(
            s.take("tab-1", "other").await,
            None,
            "other database needs a new connection"
        );
        assert_eq!(
            s.take("tab-1", "app").await,
            None,
            "and the old one is gone"
        );

        s.put("a", "x", 1).await;
        s.put("b", "x", 2).await;
        assert_eq!(s.remove("a").await, Some(1));
        assert_eq!(s.drain().await, vec![2]);
    }
}
