# dbKu patch to tiberius 0.12.3

This is tiberius 0.12.3 (MIT OR Apache-2.0, see the LICENSE files), used
through `[patch.crates-io]` in `src-tauri/Cargo.toml`, with two changes marked
`dbKu patch`:

1. `Config::allow_legacy_tls(bool)` (`src/client/config.rs`).
2. The `vendored-openssl` feature uses OpenSSL directly through
   `tokio-openssl` (`src/client/tls_stream/openssl_stream.rs`) instead of
   `opentls`. When legacy TLS is allowed it sets the minimum protocol to
   TLS 1.0 and the cipher string to `DEFAULT:@SECLEVEL=0`; otherwise OpenSSL's
   defaults apply (TLS 1.2+). Default certificate checks use the system CA
   bundle found by `openssl-probe`.

Why: SQL Server 2008, and 2012 before SP2/SP3, only speak TLS 1.0, and TDS
uses TLS for the login packet even with encryption off. rustls can't do
TLS 1.0, macOS SecureTransport (native-tls) was seen to abort the handshake
with such a server (`connection closed via error`), and opentls has no way to
lower OpenSSL 3's security level, which TLS 1.0 and 1024-bit certificates need.

To update: copy a new tiberius release over this folder and re-apply both
changes.

Removed from the vendored copy (upstream test tooling, not needed to build):
`docker/` (test certificates and keys), `docker-compose.yml`, `.envrc`, `flake.nix`,
`flake.lock`, `.github/`, `examples/`.
