use std::time::{Duration, Instant};

use futures_util::TryStreamExt;
use tiberius::{AuthMethod, ColumnData, Config, EncryptionLevel, FromSql, QueryItem};
use tokio::net::TcpStream;
use tokio::sync::Mutex;
use tokio_util::compat::{Compat, TokioAsyncWriteCompatExt};

use super::run::elapsed_ms;
use super::value::hex_blob;
use crate::error::{AppError, AppResult};
use crate::model::{
    Cell, ColumnMeta, ConnectionProfile, ExecOutcome, QueryContext, ResultSet, SslMode,
    StatementError,
};
use crate::sql::builder::quote_ident;
use crate::sql::split::Statement;
use crate::sql::Dialect;

pub(super) type Client = tiberius::Client<Compat<TcpStream>>;

const CONNECT_TIMEOUT: Duration = Duration::from_secs(20);

/// SQL Server has no connection pool here: one client for user queries and
/// one for metadata, so browsing the tree never waits behind a long query.
pub struct MssqlBackend {
    config: Config,
    read_only: bool,
    default_db: Option<String>,
    user: Mutex<Option<Client>>,
    meta: Mutex<Option<Client>>,
}

#[derive(Clone, Copy)]
pub enum Lane {
    User,
    Meta,
}

impl MssqlBackend {
    pub async fn connect(profile: &ConnectionProfile, password: Option<&str>) -> AppResult<Self> {
        let mut config = Config::new();
        config.host(&profile.host);
        config.port(if profile.port == 0 {
            1433
        } else {
            profile.port
        });
        config.authentication(AuthMethod::sql_server(
            &profile.user,
            password.unwrap_or(""),
        ));
        config.application_name("dbKu");
        config.encryption(match profile.ssl_mode {
            SslMode::Disable => EncryptionLevel::Off,
            SslMode::Prefer => EncryptionLevel::On,
            SslMode::Require => EncryptionLevel::Required,
        });
        if profile.trust_server_cert {
            config.trust_cert();
        }
        config.allow_legacy_tls(profile.legacy_tls);
        let default_db = Some(profile.database.trim().to_string()).filter(|d| !d.is_empty());
        if let Some(db) = &default_db {
            config.database(db);
        }
        let meta = open_client(&config).await?;
        Ok(Self {
            config,
            read_only: profile.read_only,
            default_db,
            user: Mutex::new(None),
            meta: Mutex::new(Some(meta)),
        })
    }

    pub fn default_db(&self) -> Option<&str> {
        self.default_db.as_deref()
    }

    fn lane(&self, lane: Lane) -> &Mutex<Option<Client>> {
        match lane {
            Lane::User => &self.user,
            Lane::Meta => &self.meta,
        }
    }

    pub async fn run(
        &self,
        ctx: &QueryContext,
        statements: &[Statement],
        max_rows: usize,
        lane: Lane,
    ) -> AppResult<ExecOutcome> {
        let started = Instant::now();
        let mut slot = self.lane(lane).lock().await;
        let mut client = match slot.take() {
            Some(c) => c,
            None => open_client(&self.config).await?,
        };
        let db = ctx
            .database
            .as_deref()
            .or(self.default_db.as_deref())
            .filter(|d| !d.is_empty());
        if let Some(db) = db {
            let use_db = format!("USE {}", quote_ident(Dialect::Mssql, db));
            client.simple_query(use_db).await?.into_results().await?;
        }

        let mut outcome = ExecOutcome::default();
        let mut keep_client = true;
        for (index, stmt) in statements.iter().enumerate() {
            match self.run_batch(&mut client, stmt, max_rows).await {
                Ok((sets, truncated)) => {
                    outcome.results.extend(sets);
                    if truncated {
                        keep_client = false;
                        let skipped = statements.len() - index - 1;
                        if skipped > 0 {
                            outcome.notices.push(format!(
                                "Result was cut at {max_rows} rows; the connection was reset, so \
                                 the remaining {skipped} batch(es) did not run."
                            ));
                        }
                        break;
                    }
                }
                Err(e) => {
                    // A server error (bad SQL, constraint) leaves the client in
                    // sync; anything else (I/O, TLS, protocol) may not.
                    keep_client = matches!(e, tiberius::error::Error::Server(_));
                    outcome.error = Some(StatementError {
                        index,
                        statement: stmt.text.clone(),
                        message: AppError::from(e).to_string(),
                    });
                    break;
                }
            }
        }
        if keep_client && self.read_only {
            // Belt and braces: nothing a read-only batch did may stick.
            let rolled_back = client
                .simple_query("IF @@TRANCOUNT > 0 ROLLBACK TRAN")
                .await;
            keep_client = match rolled_back {
                Ok(stream) => stream.into_results().await.is_ok(),
                Err(_) => false,
            };
        }
        if keep_client {
            *slot = Some(client);
        }
        outcome.elapsed_ms = elapsed_ms(started);
        Ok(outcome)
    }

    async fn run_batch(
        &self,
        client: &mut Client,
        stmt: &Statement,
        max_rows: usize,
    ) -> tiberius::Result<(Vec<ResultSet>, bool)> {
        let started = Instant::now();
        if !self.read_only && is_plain_dml(stmt) {
            let result = client.execute(stmt.text.as_str(), &[]).await?;
            let set = ResultSet {
                statement: stmt.text.clone(),
                rows_affected: Some(result.rows_affected().iter().sum()),
                elapsed_ms: elapsed_ms(started),
                ..ResultSet::default()
            };
            return Ok((vec![set], false));
        }

        let text = if self.read_only {
            format!("SET XACT_ABORT ON; BEGIN TRAN;\n{}", stmt.text)
        } else {
            stmt.text.clone()
        };
        let mut sets = Vec::new();
        let mut current: Option<ResultSet> = None;
        let mut truncated = false;
        {
            let mut stream = client.simple_query(text).await?;
            while let Some(item) = stream.try_next().await? {
                match item {
                    QueryItem::Metadata(meta) => {
                        sets.extend(current.take());
                        current = Some(ResultSet {
                            columns: meta
                                .columns()
                                .iter()
                                .map(|c| ColumnMeta {
                                    name: c.name().to_string(),
                                    type_name: format!("{:?}", c.column_type()).to_lowercase(),
                                })
                                .collect(),
                            ..ResultSet::default()
                        });
                    }
                    QueryItem::Row(row) => {
                        let set = current.get_or_insert_with(ResultSet::default);
                        if set.rows.len() >= max_rows {
                            set.truncated = true;
                            truncated = true;
                            break;
                        }
                        set.rows
                            .push(row.cells().map(|(_, data)| cell(data)).collect());
                    }
                }
            }
        }
        sets.extend(current);
        if sets.is_empty() {
            sets.push(ResultSet::default());
        }
        let ms = elapsed_ms(started);
        for set in &mut sets {
            set.statement = stmt.text.clone();
            set.elapsed_ms = ms;
        }
        Ok((sets, truncated))
    }

    /// Run grid edits as one batch that rolls back unless every UPDATE and
    /// DELETE touched exactly one row.
    pub async fn apply(
        &self,
        database: Option<&str>,
        statements: &[(String, bool)],
    ) -> AppResult<u64> {
        let mut batch = String::from("SET XACT_ABORT ON;\nBEGIN TRAN;\n");
        for (sql, must_match_one) in statements {
            batch.push_str(sql);
            batch.push_str(";\n");
            if *must_match_one {
                batch.push_str(
                    "IF @@ROWCOUNT <> 1 THROW 50001, N'An edited row no longer matches exactly \
                     one row (it was changed or the key is not unique). Nothing was saved.', 1;\n",
                );
            }
        }
        batch.push_str("COMMIT TRAN;");

        // The metadata lane never carries a user's open transaction, so the
        // edits commit on their own.
        let mut lane = self.checkout_meta(database).await?;
        let result = match lane.client.simple_query(batch).await {
            Ok(stream) => stream.into_results().await.map(|_| ()),
            Err(e) => Err(e),
        };
        let reusable = match lane.client.simple_query("SET XACT_ABORT OFF").await {
            Ok(stream) => stream.into_results().await.is_ok(),
            Err(_) => false,
        };
        if reusable {
            lane.give_back();
        }
        result?;
        Ok(statements.len() as u64)
    }

    /// A fresh client for long bulk work (dumps, imports, scripts), so the
    /// metadata client stays free for the schema tree meanwhile. Dropped
    /// when done, which also discards any session state the work left.
    pub(super) async fn open_dedicated(&self, database: Option<&str>) -> AppResult<Client> {
        let mut client = open_client(&self.config).await?;
        let target_db = database
            .or(self.default_db.as_deref())
            .filter(|d| !d.is_empty());
        if let Some(db) = target_db {
            let use_db = format!("USE {}", quote_ident(Dialect::Mssql, db));
            client.simple_query(use_db).await?.into_results().await?;
        }
        Ok(client)
    }

    pub(super) fn read_only(&self) -> bool {
        self.read_only
    }

    /// Borrow the metadata client, switched to `database` (or the default).
    /// Dropping the checkout without `give_back` discards the client, which is
    /// the safe choice whenever it may be left mid-stream or mid-transaction.
    pub(super) async fn checkout_meta(&self, database: Option<&str>) -> AppResult<Checkout<'_>> {
        let mut slot = self.meta.lock().await;
        let mut client = match slot.take() {
            Some(c) => c,
            None => open_client(&self.config).await?,
        };
        let target_db = database
            .or(self.default_db.as_deref())
            .filter(|d| !d.is_empty());
        if let Some(db) = target_db {
            let use_db = format!("USE {}", quote_ident(Dialect::Mssql, db));
            client.simple_query(use_db).await?.into_results().await?;
        }
        Ok(Checkout { slot, client })
    }
}

pub(super) struct Checkout<'a> {
    slot: tokio::sync::MutexGuard<'a, Option<Client>>,
    pub client: Client,
}

impl Checkout<'_> {
    pub fn give_back(self) {
        let Checkout { mut slot, client } = self;
        *slot = Some(client);
    }
}

/// Plain DML can go through `execute`, which reports affected-row counts.
fn is_plain_dml(stmt: &Statement) -> bool {
    let dml = matches!(
        stmt.first_word(),
        Some("INSERT" | "UPDATE" | "DELETE" | "MERGE")
    );
    let has_rows = stmt
        .words
        .iter()
        .any(|w| matches!(w.as_str(), "SELECT" | "OUTPUT" | "EXEC" | "EXECUTE"));
    dml && !has_rows
}

async fn open_client(config: &Config) -> AppResult<Client> {
    let connect = async {
        match connect_once(config.clone()).await {
            // Azure SQL may redirect to another node.
            Err(tiberius::error::Error::Routing { host, port }) => {
                let mut redirected = config.clone();
                redirected.host(&host);
                redirected.port(port);
                connect_once(redirected).await
            }
            other => other,
        }
    };
    match tokio::time::timeout(CONNECT_TIMEOUT, connect).await {
        Ok(result) => result.map_err(cert_hint),
        Err(_) => Err(AppError::Database(format!(
            "timed out connecting to {} after {}s",
            config.get_addr(),
            CONNECT_TIMEOUT.as_secs()
        ))),
    }
}

async fn connect_once(config: Config) -> tiberius::Result<Client> {
    let tcp = TcpStream::connect(config.get_addr()).await?;
    tcp.set_nodelay(true)?;
    Client::connect(config, tcp.compat_write()).await
}

fn cert_hint(e: tiberius::error::Error) -> AppError {
    let message = AppError::from(e).to_string();
    let lower = message.to_lowercase();
    if ["handshake", "protocol", "negotiation", "unexpected eof"]
        .iter()
        .any(|k| lower.contains(k))
    {
        return AppError::Database(format!(
            "{message}\n\nThe server and dbKu couldn't agree on encryption. Old servers (SQL Server \
             2008, or 2012 without updates) only speak TLS 1.0: turn on “Allow old TLS 1.0” in the \
             connection settings."
        ));
    }
    // macOS rejects the 1024-bit self-signed certificate old SQL Servers
    // generate for themselves ("The key size is not supported").
    if lower.contains("certificate") || lower.contains("key size") {
        AppError::Database(format!(
            "{message}\n\nIf this server uses a self-signed certificate, enable \
             “Trust server certificate” in the connection settings."
        ))
    } else {
        AppError::Database(message)
    }
}

pub(super) fn cell(data: &ColumnData<'static>) -> Cell {
    use chrono::{DateTime, FixedOffset, NaiveDate, NaiveDateTime, NaiveTime};
    match data {
        ColumnData::U8(v) => v.map(|x| x.to_string()),
        ColumnData::I16(v) => v.map(|x| x.to_string()),
        ColumnData::I32(v) => v.map(|x| x.to_string()),
        ColumnData::I64(v) => v.map(|x| x.to_string()),
        ColumnData::F32(v) => v.map(|x| x.to_string()),
        ColumnData::F64(v) => v.map(|x| x.to_string()),
        ColumnData::Bit(v) => v.map(|b| if b { "1" } else { "0" }.to_string()),
        ColumnData::String(v) => v.as_ref().map(|s| s.to_string()),
        ColumnData::Guid(v) => v.map(|g| g.to_string().to_uppercase()),
        ColumnData::Binary(v) => v.as_ref().map(|b| hex_blob(b)),
        ColumnData::Numeric(v) => v.map(|n| n.to_string()),
        ColumnData::Xml(v) => v.as_ref().map(|x| x.to_string()),
        ColumnData::DateTime(_) | ColumnData::SmallDateTime(_) | ColumnData::DateTime2(_) => {
            formatted::<NaiveDateTime>(data, |d| d.format("%Y-%m-%d %H:%M:%S%.f").to_string())
        }
        ColumnData::Date(_) => formatted::<NaiveDate>(data, |d| d.format("%Y-%m-%d").to_string()),
        ColumnData::Time(_) => {
            formatted::<NaiveTime>(data, |t| t.format("%H:%M:%S%.f").to_string())
        }
        ColumnData::DateTimeOffset(_) => formatted::<DateTime<FixedOffset>>(data, |d| {
            d.format("%Y-%m-%d %H:%M:%S%.f %:z").to_string()
        }),
    }
}

fn formatted<'a, T: FromSql<'a>>(data: &'a ColumnData<'static>, fmt: impl Fn(T) -> String) -> Cell {
    match T::from_sql(data) {
        Ok(v) => v.map(fmt),
        Err(e) => Some(format!("<{e}>")),
    }
}
