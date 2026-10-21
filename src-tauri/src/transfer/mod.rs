//! Moving data in and out: SQL dumps, CSV import, running SQL files.
//!
//! Everything streams between the database and a file on the Rust side;
//! large data never crosses the IPC bridge.

pub mod csv_import;
pub mod ddl;
pub mod dump;
pub mod script;

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use crate::sql::builder::quote_literal;
use crate::sql::Dialect;

/// A value read for re-insertion, keeping just enough type information to
/// write a literal the target database accepts.
#[derive(Debug, Clone, PartialEq)]
pub enum DumpValue {
    Null,
    /// Written without quotes. Only plain decimal numbers end up here.
    Number(String),
    Text(String),
    Bytes(Vec<u8>),
}

impl DumpValue {
    /// `Number` when the text is a plain decimal, otherwise `Text`
    /// (so `NaN`, `Infinity` or `1e5x` stay quoted).
    pub fn numeric(text: String) -> Self {
        if is_plain_number(&text) {
            DumpValue::Number(text)
        } else {
            DumpValue::Text(text)
        }
    }
}

fn is_plain_number(s: &str) -> bool {
    let digits = |p: &str| !p.is_empty() && p.bytes().all(|b| b.is_ascii_digit());
    let body = s.strip_prefix('-').unwrap_or(s);
    let (mantissa, exponent) = match body.split_once(['e', 'E']) {
        Some((m, e)) => (m, Some(e)),
        None => (body, None),
    };
    let mantissa_ok = match mantissa.split_once('.') {
        Some((int, frac)) => digits(int) && digits(frac),
        None => digits(mantissa),
    };
    let exponent_ok = match exponent {
        Some(e) => digits(e.strip_prefix(['+', '-']).unwrap_or(e)),
        None => true,
    };
    mantissa_ok && exponent_ok
}

pub fn sql_literal(d: Dialect, v: &DumpValue) -> String {
    match v {
        DumpValue::Null => "NULL".to_string(),
        DumpValue::Number(n) => n.clone(),
        DumpValue::Text(t) => quote_literal(d, t),
        DumpValue::Bytes(b) => {
            let hex: String = b.iter().map(|x| format!("{x:02X}")).collect();
            match d {
                Dialect::Postgres => format!("'\\x{hex}'"),
                Dialect::Mysql | Dialect::Sqlite => format!("X'{hex}'"),
                Dialect::Mssql => format!("0x{hex}"),
            }
        }
    }
}

/// Reports progress and asks whether the user cancelled.
pub trait Progress: Send + Sync {
    fn report(&self, done: u64, total: Option<u64>, label: &str);
    fn cancelled(&self) -> bool;
}

/// For callers that don't show progress (and tests).
pub struct Silent;

impl Progress for Silent {
    fn report(&self, _: u64, _: Option<u64>, _: &str) {}
    fn cancelled(&self) -> bool {
        false
    }
}

/// Cancel flags for running transfer tasks, by task id.
#[derive(Default, Clone)]
pub struct Tasks {
    flags: Arc<Mutex<HashMap<String, Arc<AtomicBool>>>>,
}

impl Tasks {
    pub fn start(&self, id: &str) -> Arc<AtomicBool> {
        let flag = Arc::new(AtomicBool::new(false));
        if let Ok(mut m) = self.flags.lock() {
            m.insert(id.to_string(), Arc::clone(&flag));
        }
        flag
    }

    pub fn cancel(&self, id: &str) -> bool {
        match self.flags.lock().ok().and_then(|m| m.get(id).cloned()) {
            Some(flag) => {
                flag.store(true, Ordering::SeqCst);
                true
            }
            None => false,
        }
    }

    pub fn finish(&self, id: &str) {
        if let Ok(mut m) = self.flags.lock() {
            m.remove(id);
        }
    }
}

pub const CANCELLED: &str = "Cancelled.";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_plain_decimals_are_unquoted() {
        for n in ["0", "-12", "3.14", "1e5", "-2.5E-3", "00123"] {
            assert_eq!(
                DumpValue::numeric(n.into()),
                DumpValue::Number(n.into()),
                "{n}"
            );
        }
        for t in [
            "NaN", "Infinity", "-", "1.", ".5", "1e", "12a", "1 ", "", "1;DROP",
        ] {
            assert_eq!(
                DumpValue::numeric(t.into()),
                DumpValue::Text(t.into()),
                "{t:?}"
            );
        }
    }

    #[test]
    fn renders_literals_per_dialect() {
        let bytes = DumpValue::Bytes(vec![0x00, 0xAB]);
        assert_eq!(sql_literal(Dialect::Postgres, &bytes), "'\\x00AB'");
        assert_eq!(sql_literal(Dialect::Mysql, &bytes), "X'00AB'");
        assert_eq!(sql_literal(Dialect::Mssql, &bytes), "0x00AB");
        assert_eq!(sql_literal(Dialect::Sqlite, &DumpValue::Null), "NULL");
        assert_eq!(
            sql_literal(Dialect::Mssql, &DumpValue::Text("it's".into())),
            "N'it''s'"
        );
        assert_eq!(
            sql_literal(Dialect::Postgres, &DumpValue::Number("-1.5".into())),
            "-1.5"
        );
    }

    #[test]
    fn cancel_flags_are_per_task() {
        let tasks = Tasks::default();
        let a = tasks.start("a");
        let b = tasks.start("b");
        assert!(tasks.cancel("a"));
        assert!(a.load(Ordering::SeqCst) && !b.load(Ordering::SeqCst));
        tasks.finish("a");
        assert!(!tasks.cancel("a"));
    }
}
