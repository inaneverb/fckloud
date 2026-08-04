mod client;

use {
    self::client::ExportClient,
    anyhow::{Context as _, Result},
    opentelemetry::{KeyValue, global, metrics::Meter, trace::TracerProvider as _},
    opentelemetry_otlp::{MetricExporter, SpanExporter, WithHttpConfig as _},
    opentelemetry_sdk::{
        Resource,
        metrics::{PeriodicReader, SdkMeterProvider},
        trace::SdkTracerProvider,
    },
    opentelemetry_semantic_conventions::attribute::SERVICE_VERSION,
    std::{env, time::Duration},
    tracing::{Subscriber, debug, info, warn},
    tracing_subscriber::{
        Layer,
        filter::{LevelFilter, Targets},
        registry::LookupSpan,
    },
};

const SCOPE: &str = env!("CARGO_PKG_NAME");

// A collector that has gone away must not hold the pod past its grace period;
// SIGTERM handling exists precisely so deletion does not wait that long.
const SHUTDOWN_TIMEOUT: Duration = Duration::from_secs(3);

const DISABLE_VAR: &str = "OTEL_SDK_DISABLED";
const SERVICE_NAME_VAR: &str = "OTEL_SERVICE_NAME";

// An endpoint is the on switch. There is no assumed address: this pod runs on
// the host's network, and guessing a port there means pushing a node's
// telemetry at whatever else happens to be listening on it.
const SHARED_ENDPOINT_VAR: &str = "OTEL_EXPORTER_OTLP_ENDPOINT";

const TRACES: Signal = Signal {
    exporter_var: "OTEL_TRACES_EXPORTER",
    endpoint_var: "OTEL_EXPORTER_OTLP_TRACES_ENDPOINT",
};

const METRICS: Signal = Signal {
    exporter_var: "OTEL_METRICS_EXPORTER",
    endpoint_var: "OTEL_EXPORTER_OTLP_METRICS_ENDPOINT",
};

/// A layer that turns `tracing` spans into OTLP, or nothing at all.
type ExportLayer<S> = Option<Box<dyn Layer<S> + Send + Sync>>;

/// One signal's share of the standard environment: what turns it off on its
/// own, and where it goes when it is on.
struct Signal {
    exporter_var: &'static str,
    endpoint_var: &'static str,
}

/// Owns the exporters for as long as the process lives and flushes them on the
/// way out. Dropping it without [`Self::shutdown`] loses the last batch.
#[derive(Default)]
pub struct Guard {
    tracer: Option<SdkTracerProvider>,
    meter: Option<SdkMeterProvider>,
    outcome: Outcome,
}

#[derive(Default)]
enum Outcome {
    #[default]
    Off,
    On {
        endpoint: String,
        signals: &'static str,
    },
    Unavailable(String),
}

/// What [`try_install`] managed to build.
struct Installed {
    tracer: Option<SdkTracerProvider>,
    meter: Option<SdkMeterProvider>,
    endpoint: String,
    signals: &'static str,
}

/// The meter every instrument in this crate is built from.
///
/// Safe whether or not [`install`] set anything up: with no provider the
/// global one is a no-op, and so is every instrument made from it.
pub fn meter() -> Meter {
    global::meter(SCOPE)
}

/// Builds the exporters the environment asked for, if it asked for any.
///
/// Never fails: telemetry is not load-bearing here, and an exporter that
/// cannot be built costs traces, not the node's `ExternalIP`.
pub fn install<S>() -> (ExportLayer<S>, Guard)
where
    S: Subscriber + for<'a> LookupSpan<'a> + Send + Sync,
{
    let installed = match try_install() {
        Ok(Some(installed)) => installed,
        Ok(None) => return (None, Guard::default()),

        Err(err) => {
            let guard = Guard {
                outcome: Outcome::Unavailable(format!("{err:#}")),
                ..Guard::default()
            };

            return (None, guard);
        }
    };

    // The instruments reach their provider through the global one; the tracer
    // is handed to the layer directly and needs no such thing.
    if let Some(meter) = &installed.meter {
        global::set_meter_provider(meter.clone());
    }

    let layer = installed.tracer.as_ref().map(|provider| {
        tracing_opentelemetry::layer()
            .with_tracer(provider.tracer(SCOPE))
            .with_filter(export_filter())
            .boxed()
    });

    let guard = Guard {
        tracer: installed.tracer,
        meter: installed.meter,
        outcome: Outcome::On {
            endpoint: installed.endpoint,
            signals: installed.signals,
        },
    };

    (layer, guard)
}

impl Guard {
    /// Says what [`install`] decided. Separate from it because that runs while
    /// the subscriber is still being assembled, where a log has nowhere to go.
    pub fn announce(&self) {
        match &self.outcome {
            Outcome::Off => debug!("no OTLP endpoint is configured, telemetry stays here"),
            Outcome::On { endpoint, signals } => info!(endpoint, signals, "exporting over OTLP"),
            Outcome::Unavailable(err) => {
                warn!(err, "cannot start the OTLP exporter, running without it");
            }
        }
    }

    /// Flushes whatever is still queued, and gives up rather than wait forever.
    pub fn shutdown(self) {
        if let Some(tracer) = self.tracer
            && let Err(err) = tracer.shutdown_with_timeout(SHUTDOWN_TIMEOUT)
        {
            debug!(%err, "the last trace flush did not finish");
        }

        // No timeout to give: the metric reader hard-codes its own five
        // seconds and ignores the argument. The client's connect timeout is
        // what actually bounds this.
        if let Some(meter) = self.meter
            && let Err(err) = meter.shutdown()
        {
            debug!(%err, "the last metric flush did not finish");
        }
    }
}

fn try_install() -> Result<Option<Installed>> {
    let want_traces = TRACES.endpoint();
    let want_metrics = METRICS.endpoint();

    let endpoint = match (&want_traces, &want_metrics) {
        (Some(endpoint), _) | (None, Some(endpoint)) => endpoint.clone(),
        (None, None) => return Ok(None),
    };

    let signals = match (want_traces.is_some(), want_metrics.is_some()) {
        (true, true) => "traces and metrics",
        (true, false) => "traces",
        _ => "metrics",
    };

    let client = ExportClient::new().context("cannot build the export client")?;

    let tracer = want_traces
        .map(|_| build_tracer(client.clone()))
        .transpose()
        .context("cannot build the OTLP span exporter")?;

    let meter = want_metrics
        .map(|_| build_meter(client))
        .transpose()
        .context("cannot build the OTLP metric exporter")?;

    Ok(Some(Installed {
        tracer,
        meter,
        endpoint,
        signals,
    }))
}

// The endpoint is deliberately not passed to either builder: the SDK reads it
// from the environment itself, and only it knows that the per-signal variable
// is used verbatim while the shared one gets "/v1/traces" appended.
fn build_tracer(client: ExportClient) -> Result<SdkTracerProvider> {
    let exporter = SpanExporter::builder()
        .with_http()
        .with_http_client(client)
        .build()?;

    let provider = SdkTracerProvider::builder()
        .with_batch_exporter(exporter)
        .with_resource(resource())
        .build();

    Ok(provider)
}

fn build_meter(client: ExportClient) -> Result<SdkMeterProvider> {
    let exporter = MetricExporter::builder()
        .with_http()
        .with_http_client(client)
        .build()?;

    let provider = SdkMeterProvider::builder()
        .with_reader(PeriodicReader::builder(exporter).build())
        .with_resource(resource())
        .build();

    Ok(provider)
}

fn resource() -> Resource {
    let version = KeyValue::new(SERVICE_VERSION, env!("CARGO_PKG_VERSION"));
    let mut builder = Resource::builder().with_attribute(version);

    if env::var_os(SERVICE_NAME_VAR).is_none() {
        builder = builder.with_service_name(SCOPE);
    }

    builder.build()
}

// The SDK's own diagnostics are `tracing` events like any other. Exporting
// them would mean an export failure describing itself into the queue that
// just failed to drain.
fn export_filter() -> Targets {
    Targets::new()
        .with_default(LevelFilter::INFO)
        .with_target("opentelemetry", LevelFilter::OFF)
}

impl Signal {
    /// Where this signal should go, or [`None`] if it should not be exported.
    fn endpoint(&self) -> Option<String> {
        if is_env_true(DISABLE_VAR) || is_env_none(self.exporter_var) {
            return None;
        }

        [self.endpoint_var, SHARED_ENDPOINT_VAR]
            .iter()
            .filter_map(|name| env::var(name).ok())
            .map(|value| value.trim().to_owned())
            .find(|value| !value.is_empty())
    }
}

fn is_env_true(name: &str) -> bool {
    env::var(name).is_ok_and(|value| value.trim().eq_ignore_ascii_case("true"))
}

fn is_env_none(name: &str) -> bool {
    env::var(name).is_ok_and(|value| value.trim().eq_ignore_ascii_case("none"))
}
