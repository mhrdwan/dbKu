//! Manual check of the SQL Server TLS path against a local fake server that only
//! speaks TLS 1.0 with a 1024-bit certificate, like SQL Server 2012 RTM:
//!
//! ```text
//! openssl req -x509 -newkey rsa:1024 -nodes -subj /CN=fake -keyout k.pem -out c.pem
//! python3 tests/support/fake_tds.py 14330 c.pem k.pem &
//! DBKU_FAKE_TDS_PORT=14330 cargo test --test mssql_tls_local -- --ignored --nocapture
//! ```
//!
//! The server should print "TLS handshake done". With DBKU_FAKE_LEGACY=0 the
//! client must refuse TLS 1.0 instead.

use dbku_lib::db::Session;
use dbku_lib::model::*;

#[tokio::test]
#[ignore]
async fn legacy_tls_handshake_reaches_login() {
    let port: u16 = std::env::var("DBKU_FAKE_TDS_PORT")
        .expect("port")
        .parse()
        .unwrap();
    let profile = ConnectionProfile {
        id: "fake".into(),
        name: "fake".into(),
        driver: Driver::Mssql,
        host: "127.0.0.1".into(),
        port,
        user: "nobody".into(),
        database: String::new(),
        ssl_mode: SslMode::Prefer,
        trust_server_cert: true,
        legacy_tls: std::env::var("DBKU_FAKE_LEGACY").as_deref() != Ok("0"),
        read_only: true,
        color: None,
        group: None,
        save_password: false,
    };
    let err = Session::open(profile, Some("x"))
        .await
        .err()
        .map(|e| e.to_string());
    println!("CLIENT RESULT: {err:?}");
}
