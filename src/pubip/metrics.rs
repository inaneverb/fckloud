use {
    crate::{
        pubip::{HttpProvider, error::FetchError},
        telemetry::meter,
    },
    opentelemetry::{
        KeyValue,
        metrics::{Gauge, Histogram},
    },
    std::{sync::LazyLock, time::Duration},
};

// Milliseconds, where the semantic conventions ask for seconds. The SDK's
// default bucket boundaries are the millisecond ones, and in seconds every
// provider call lands in the first bucket and the histogram says nothing.
static REQUEST_DURATION: LazyLock<Histogram<f64>> = LazyLock::new(|| {
    meter()
        .f64_histogram("fckloud.provider.request.duration")
        .with_unit("ms")
        .with_description("How long a provider took to answer, and whether it did")
        .build()
});

static CONSENSUS_ADDRESSES: LazyLock<Gauge<u64>> = LazyLock::new(|| {
    meter()
        .u64_gauge("fckloud.consensus.addresses")
        .with_unit("{address}")
        .with_description(
            "Distinct addresses the last round weighed, by whether trust confirmed them",
        )
        .build()
});

/// Records what one provider cost and whether it was usable.
///
/// The address it reported is not among the attributes, and must not become
/// one: a label is a time series kept forever, and there is no bound on what
/// a misbehaving provider can put in it.
pub fn record_request(provider: HttpProvider, elapsed: Duration, failure: Option<&FetchError>) {
    let mut attributes = vec![KeyValue::new("fckloud.provider", provider.host())];

    if let Some(err) = failure {
        attributes.push(KeyValue::new("error.type", err.as_error_type()));

        if let Some(status) = err.status_code() {
            attributes.push(KeyValue::new(
                "http.response.status_code",
                i64::from(status),
            ));
        }
    }

    REQUEST_DURATION.record(elapsed.as_secs_f64() * 1000.0, &attributes);
}

/// Records how the round divided: how many addresses cleared the threshold and
/// how many fell short. Both zero means nobody answered.
pub fn record_consensus(confirmed: usize, unconfirmed: usize) {
    const STATE: &str = "fckloud.consensus.state";

    CONSENSUS_ADDRESSES.record(confirmed as u64, &[KeyValue::new(STATE, "confirmed")]);
    CONSENSUS_ADDRESSES.record(unconfirmed as u64, &[KeyValue::new(STATE, "unconfirmed")]);
}
