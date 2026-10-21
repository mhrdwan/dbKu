pub mod builder;
pub mod guard;
pub mod split;

use crate::model::Driver;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Dialect {
    Postgres,
    Mysql,
    Sqlite,
    Mssql,
}

impl From<Driver> for Dialect {
    fn from(d: Driver) -> Self {
        match d {
            Driver::Postgres => Dialect::Postgres,
            Driver::Mysql => Dialect::Mysql,
            Driver::Sqlite => Dialect::Sqlite,
            Driver::Mssql => Dialect::Mssql,
        }
    }
}
