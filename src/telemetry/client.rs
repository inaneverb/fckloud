use {
    anyhow::{Context as _, Result},
    async_trait::async_trait,
    hyper_rustls::{HttpsConnector, HttpsConnectorBuilder},
    hyper_util::client::legacy::connect::HttpConnector,
    opentelemetry_http::{Bytes, HttpClient, HttpError, Request, Response, hyper::HyperClient},
    std::{
        fmt,
        sync::{
            Arc,
            atomic::{AtomicBool, Ordering},
        },
        time::Duration,
    },
    tokio::runtime::{Builder as RuntimeBuilder, Handle, Runtime},
    tracing::{debug, info, warn},
};

const CONNECT_TIMEOUT: Duration = Duration::from_secs(2);
const EXPORT_TIMEOUT: Duration = Duration::from_secs(10);

/// The HTTP client the OTLP exporters send through, and the only place that
/// learns whether the collector is answering.
///
/// Cloned rather than rebuilt per signal, so traces and metrics share one
/// connection pool, one runtime, and one opinion about the collector's health.
#[derive(Clone)]
pub struct ExportClient(Arc<Inner>);

struct Inner {
    runtime: Runtime,
    hyper: HyperClient<HttpsConnector<HttpConnector>>,
    failing: AtomicBool,
}

impl ExportClient {
    pub fn new() -> Result<Self> {
        // Without a connect timeout of its own, a black-holed endpoint is only
        // bounded by the export timeout, and the flush on the way out would
        // spend all of it against a collector that is not coming back.
        let mut plain = HttpConnector::new();
        plain.set_connect_timeout(Some(CONNECT_TIMEOUT));
        plain.enforce_http(false);

        let connector = HttpsConnectorBuilder::new()
            .with_native_roots()
            .context("cannot read the system certificate store")?
            .https_or_http()
            .enable_http1()
            .wrap_connector(plain);

        // The SDK drives exports with `futures_executor::block_on` on a bare
        // thread, where hyper finds no reactor to register with. A runtime of
        // its own also keeps a stalled collector off the one running the ticks.
        let runtime = RuntimeBuilder::new_current_thread()
            .enable_all()
            .build()
            .context("cannot build the exporter runtime")?;

        Ok(Self(Arc::new(Inner {
            runtime,
            hyper: HyperClient::new(connector, EXPORT_TIMEOUT, None),
            failing: AtomicBool::new(false),
        })))
    }
}

impl Inner {
    // Complain once, then keep quiet until an export lands. A collector that
    // comes back and goes away again has earned a second warning; the hundred
    // failures in between have not.
    fn observe(&self, failure: Option<&HttpError>) {
        match failure {
            Some(err) if self.failing.swap(true, Ordering::Relaxed) => {
                debug!(%err, "still cannot export telemetry");
            }
            Some(err) => {
                warn!(%err, "cannot export telemetry, quiet until one lands");
            }
            None if self.failing.swap(false, Ordering::Relaxed) => {
                info!("telemetry export works again");
            }
            None => (),
        }
    }
}

#[async_trait]
impl HttpClient for ExportClient {
    async fn send_bytes(&self, request: Request<Bytes>) -> Result<Response<Bytes>, HttpError> {
        let sending = self.0.hyper.send_bytes(request);

        let result = match Handle::try_current() {
            Ok(_) => sending.await,
            Err(_) => self.0.runtime.block_on(sending),
        };

        self.0.observe(result.as_ref().err());
        result
    }
}

impl fmt::Debug for ExportClient {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("ExportClient")
    }
}
