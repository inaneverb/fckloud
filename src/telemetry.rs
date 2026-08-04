mod client;

use {
    self::client::ExportClient,
    anyhow::{Context as _, Result},
    opentelemetry::{KeyValue, trace::TracerProvider as _},
    opentelemetry_otlp::{SpanExporter, WithHttpConfig as _},
    opentelemetry_sdk::{Resource, trace::SdkTracerProvider},
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
const SHUTDOWN_TIMEOUT: Duration = Duration::from_secs(5);

const DISABLE_VAR: &str = "OTEL_SDK_DISABLED";
const SERVICE_NAME_VAR: &str = "OTEL_SERVICE_NAME";
const TRACES_EXPORTER_VAR: &str = "OTEL_TRACES_EXPORTER";

// An endpoint is the on switch. There is no assumed address: this pod runs on
// the host's network, and guessing a port there means pushing a node's traces
// at whatever else happens to be listening on it.
const ENDPOINT_VARS: [&str; 2] = [
    "OTEL_EXPORTER_OTLP_TRACES_ENDPOINT",
    "OTEL_EXPORTER_OTLP_ENDPOINT",
];

/// A layer that turns `tracing` spans into OTLP, or nothing at all.
type ExportLayer<S> = Option<Box<dyn Layer<S> + Send + Sync>>;

/// Owns the exporter for as long as the process lives and flushes it on the
/// way out. Dropping it without [`Self::shutdown`] loses the last batch.
#[derive(Default)]
pub struct Guard {
    provider: Option<SdkTracerProvider>,
    outcome: Outcome,
}

#[derive(Default)]
enum Outcome {
    #[default]
    Off,
    Exporting(String),
    Unavailable(String),
}

/// Builds the export layer, if the environment asked for one.
///
/// Never fails: telemetry is not load-bearing here, and an exporter that
/// cannot be built costs traces, not the node's `ExternalIP`.
pub fn install<S>() -> (ExportLayer<S>, Guard)
where
    S: Subscriber + for<'a> LookupSpan<'a> + Send + Sync,
{
    let Some(endpoint) = wanted_endpoint() else {
        return (None, Guard::default());
    };

    match build_provider() {
        Ok(provider) => {
            let layer = tracing_opentelemetry::layer()
                .with_tracer(provider.tracer(SCOPE))
                .with_filter(export_filter())
                .boxed();

            let guard = Guard {
                provider: Some(provider),
                outcome: Outcome::Exporting(endpoint),
            };

            (Some(layer), guard)
        }

        Err(err) => {
            let guard = Guard {
                provider: None,
                outcome: Outcome::Unavailable(format!("{err:#}")),
            };

            (None, guard)
        }
    }
}

impl Guard {
    /// Says what [`install`] decided. Separate from it because that runs while
    /// the subscriber is still being assembled, where a log has nowhere to go.
    pub fn announce(&self) {
        match &self.outcome {
            Outcome::Off => debug!("no OTLP endpoint is configured, traces stay in this process"),
            Outcome::Exporting(endpoint) => info!(endpoint, "exporting traces over OTLP"),
            Outcome::Unavailable(err) => {
                warn!(
                    err,
                    "cannot start the OTLP exporter, running without traces"
                );
            }
        }
    }

    /// Flushes whatever is still queued, and gives up rather than wait forever.
    pub fn shutdown(self) {
        let Some(provider) = self.provider else {
            return;
        };

        if let Err(err) = provider.shutdown_with_timeout(SHUTDOWN_TIMEOUT) {
            debug!(%err, "the last telemetry flush did not finish");
        }
    }
}

fn build_provider() -> Result<SdkTracerProvider> {
    // The endpoint is deliberately not passed in: the SDK reads it from the
    // environment itself, and only it knows that the per-signal variable is
    // used verbatim while the shared one gets "/v1/traces" appended.
    let exporter = SpanExporter::builder()
        .with_http()
        .with_http_client(ExportClient::new().context("cannot build the export client")?)
        .build()
        .context("cannot build the OTLP span exporter")?;

    let provider = SdkTracerProvider::builder()
        .with_batch_exporter(exporter)
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

/// The configured endpoint, or [`None`] if this process should not export.
fn wanted_endpoint() -> Option<String> {
    if is_env_true(DISABLE_VAR) || is_env_none(TRACES_EXPORTER_VAR) {
        return None;
    }

    ENDPOINT_VARS
        .iter()
        .filter_map(|name| env::var(name).ok())
        .map(|value| value.trim().to_owned())
        .find(|value| !value.is_empty())
}

fn is_env_true(name: &str) -> bool {
    env::var(name).is_ok_and(|value| value.trim().eq_ignore_ascii_case("true"))
}

fn is_env_none(name: &str) -> bool {
    env::var(name).is_ok_and(|value| value.trim().eq_ignore_ascii_case("none"))
}
