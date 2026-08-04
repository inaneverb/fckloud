use {
    crate::{node::AddrStatus, telemetry::meter},
    anyhow::Error,
    opentelemetry::{
        KeyValue,
        metrics::{Counter, Gauge, Histogram},
    },
    std::{collections::BTreeMap, net::IpAddr, sync::LazyLock, time::Duration},
};

// Milliseconds, for the same reason as the provider histogram: the default
// bucket boundaries are the millisecond ones.
static REQUEST_DURATION: LazyLock<Histogram<f64>> = LazyLock::new(|| {
    meter()
        .f64_histogram("fckloud.k8s.request.duration")
        .with_unit("ms")
        .with_description("How long the API server took, and whether it answered")
        .build()
});

static ADDRESS_CHANGES: LazyLock<Counter<u64>> = LazyLock::new(|| {
    meter()
        .u64_counter("fckloud.node.address.changes")
        .with_unit("{change}")
        .with_description("What each reconciliation did to the node's ExternalIPs")
        .build()
});

static EXTERNAL_IPS: LazyLock<Gauge<u64>> = LazyLock::new(|| {
    meter()
        .u64_gauge("fckloud.node.external_ips")
        .with_unit("{address}")
        .with_description("ExternalIPs the node carries after the last reconciliation")
        .build()
});

/// Records what one call to the API server cost and whether it worked.
pub fn record_request(operation: &'static str, elapsed: Duration, failure: Option<&Error>) {
    let mut attributes = vec![KeyValue::new("fckloud.k8s.operation", operation)];

    if let Some(err) = failure {
        attributes.push(KeyValue::new("error.type", error_type(err)));

        if let Some(status) = status_code(err) {
            attributes.push(KeyValue::new(
                "http.response.status_code",
                i64::from(status),
            ));
        }
    }

    REQUEST_DURATION.record(elapsed.as_secs_f64() * 1000.0, &attributes);
}

/// Records what the reconciliation decided. A rising `removed` is the one
/// worth waking somebody for: it means addresses are being torn off a live
/// node, which is what `--strict` does when consensus stops agreeing.
pub fn record_report(report: &BTreeMap<IpAddr, AddrStatus>) {
    const STATUS: &str = "fckloud.address.status";

    let mut attached = 0;
    for status in report.values() {
        let name = match status {
            AddrStatus::New => "added",
            AddrStatus::Skipped => "kept",
            AddrStatus::Removed => "removed",
        };

        if !status.is_removed() {
            attached += 1;
        }

        ADDRESS_CHANGES.add(1, &[KeyValue::new(STATUS, name)]);
    }

    EXTERNAL_IPS.record(attached, &[]);
}

// Coarse on purpose. Which of the API server's many ways to say no this was
// belongs in the log; a label only has to separate "it refused us" from "we
// could not reach it", because those are different pages at 3am.
fn error_type(err: &Error) -> &'static str {
    match err.downcast_ref::<kube::Error>() {
        Some(kube::Error::Api(_)) => "api",
        Some(kube::Error::HyperError(_) | kube::Error::Service(_)) => "connection",
        Some(_) => "kube",
        None => "other",
    }
}

fn status_code(err: &Error) -> Option<u16> {
    match err.downcast_ref::<kube::Error>() {
        Some(kube::Error::Api(response)) => Some(response.code),
        _ => None,
    }
}
