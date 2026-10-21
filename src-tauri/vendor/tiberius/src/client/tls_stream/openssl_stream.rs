//! dbKu patch: TLS through OpenSSL (vendored), replacing opentls so the
//! security level can be lowered for servers that only speak TLS 1.0.
//! See DBKU_PATCH.md.

use crate::client::{config::Config, TrustConfig};
use futures_util::io::{AsyncRead, AsyncWrite};
use openssl::ssl::{SslConnector, SslMethod, SslVerifyMode, SslVersion};
use std::{
    io,
    pin::Pin,
    task::{Context, Poll},
};
use tokio_util::compat::{Compat, FuturesAsyncReadCompatExt, TokioAsyncReadCompatExt};
use tracing::{event, Level};

pub(crate) struct TlsStream<S>(Compat<tokio_openssl::SslStream<Compat<S>>>);

impl<S> TlsStream<S> {
    pub(crate) fn get_mut(&mut self) -> &mut S {
        self.0.get_mut().get_mut().get_mut()
    }
}

impl<S: AsyncRead + AsyncWrite + Unpin> AsyncRead for TlsStream<S> {
    fn poll_read(self: Pin<&mut Self>, cx: &mut Context<'_>, buf: &mut [u8]) -> Poll<io::Result<usize>> {
        Pin::new(&mut self.get_mut().0).poll_read(cx, buf)
    }
}

impl<S: AsyncRead + AsyncWrite + Unpin> AsyncWrite for TlsStream<S> {
    fn poll_write(self: Pin<&mut Self>, cx: &mut Context<'_>, buf: &[u8]) -> Poll<io::Result<usize>> {
        Pin::new(&mut self.get_mut().0).poll_write(cx, buf)
    }

    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.get_mut().0).poll_flush(cx)
    }

    fn poll_close(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.get_mut().0).poll_close(cx)
    }
}

pub(crate) async fn create_tls_stream<S: AsyncRead + AsyncWrite + Unpin + Send>(
    config: &Config,
    stream: S,
) -> crate::Result<TlsStream<S>> {
    let mut builder = SslConnector::builder(SslMethod::tls_client())?;

    if config.legacy_tls {
        event!(Level::WARN, "Allowing TLS 1.0 for an old server.");
        builder.set_min_proto_version(Some(SslVersion::TLS1))?;
        // OpenSSL 3 refuses TLS 1.0, SHA-1 and 1024-bit keys above level 0.
        builder.set_cipher_list("DEFAULT:@SECLEVEL=0")?;
    }

    let mut verify = true;
    match &config.trust {
        TrustConfig::CaCertificateLocation(path) => builder.set_ca_file(path)?,
        TrustConfig::TrustAll => {
            event!(Level::WARN, "Trusting the server certificate without validation.");
            builder.set_verify(SslVerifyMode::NONE);
            verify = false;
        }
        TrustConfig::Default => {
            // Vendored OpenSSL has no built-in roots; use the system bundle.
            let probe = openssl_probe::probe();
            if let Some(file) = probe.cert_file {
                builder.set_ca_file(file)?;
            } else if let Some(dir) = probe.cert_dir {
                builder.load_verify_locations(None, Some(&dir))?;
            }
        }
    }

    let ssl = builder
        .build()
        .configure()?
        .verify_hostname(verify)
        .use_server_name_indication(verify)
        .into_ssl(&config.get_host())?;
    let mut tls = tokio_openssl::SslStream::new(ssl, stream.compat())?;
    Pin::new(&mut tls).connect().await?;
    Ok(TlsStream(tls.compat()))
}
