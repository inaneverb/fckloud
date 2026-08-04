use {
    crate::{
        pubip::{HttpProvider, error::FetchError},
        telemetry::meter,
    },
    opentelemetry::{
        KeyValue,
        metrics::{AsyncInstrument, Gauge, Histogram, ObservableGauge},
    },
    std::{
        array,
        collections::HashMap,
        sync::{LazyLock, Mutex, MutexGuard, PoisonError},
        time::{Duration, Instant},
    },
};

// How far back each rolling count reaches. A month is thirty days here: the
// alternative is a calendar, and nothing downstream is worth that.
const WINDOWS: [(&str, Duration); 3] = [
    ("1h", Duration::from_hours(1)),
    ("24h", Duration::from_hours(24)),
    ("30d", Duration::from_hours(24 * 30)),
];

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

// These windows are process-local and start empty after a restart, which is
// the price of reading them straight off the metric. The authoritative answer
// over a long stretch is still increase() over the histogram above.
static FAILURE_COUNT: LazyLock<ObservableGauge<u64>> = LazyLock::new(|| {
    meter()
        .u64_observable_gauge("fckloud.provider.failures")
        .with_unit("{failure}")
        .with_description(
            "Failures per provider so far in each window, which restarts once it elapses",
        )
        .with_callback(|observer| failures().observe(observer))
        .build()
});

static FAILURES: LazyLock<Mutex<Failures>> = LazyLock::new(|| Mutex::new(Failures::new()));

struct Failures {
    windows: [Window; WINDOWS.len()],
}

struct Window {
    started: Instant,
    counts: HashMap<HttpProvider, u64>,
}

/// Puts the enabled providers on the board at zero and registers the gauge
/// before the first export, so that a provider which never fails reads as
/// healthy rather than going missing.
pub fn register(providers: &[HttpProvider]) {
    failures().seed(providers);
    LazyLock::force(&FAILURE_COUNT);
}

fn failures() -> MutexGuard<'static, Failures> {
    // A panic while counting failures must not stop the operator counting
    // them; the worst a poisoned lock costs here is one lost tally.
    FAILURES.lock().unwrap_or_else(PoisonError::into_inner)
}

impl Failures {
    fn new() -> Self {
        let started = Instant::now();
        Self {
            windows: array::from_fn(|_| Window {
                started,
                counts: HashMap::new(),
            }),
        }
    }

    fn seed(&mut self, providers: &[HttpProvider]) {
        for window in &mut self.windows {
            for provider in providers {
                window.counts.entry(*provider).or_insert(0);
            }
        }
    }

    fn count(&mut self, provider: HttpProvider) {
        let now = Instant::now();

        for (window, (_, span)) in self.windows.iter_mut().zip(WINDOWS) {
            window.roll(now, span);
            *window.counts.entry(provider).or_insert(0) += 1;
        }
    }

    fn observe(&mut self, observer: &dyn AsyncInstrument<u64>) {
        let now = Instant::now();

        for (window, (period, span)) in self.windows.iter_mut().zip(WINDOWS) {
            window.roll(now, span);

            for (provider, count) in &window.counts {
                observer.observe(
                    *count,
                    &[
                        KeyValue::new("fckloud.provider", provider.host()),
                        KeyValue::new("fckloud.period", period),
                    ],
                );
            }
        }
    }
}

impl Window {
    // Zeroed rather than emptied: a provider seeded at the start should keep
    // reporting zero across the boundary instead of vanishing from the metric.
    fn roll(&mut self, now: Instant, span: Duration) {
        if now.duration_since(self.started) >= span {
            self.counts.values_mut().for_each(|count| *count = 0);
            self.started = now;
        }
    }
}

/// Records what one provider cost and whether it was usable.
///
/// The address it reported is not among the attributes, and must not become
/// one: a label is a time series kept forever, and there is no bound on what
/// a misbehaving provider can put in it.
pub fn record_request(provider: HttpProvider, elapsed: Duration, failure: Option<&FetchError>) {
    let mut attributes = vec![KeyValue::new("fckloud.provider", provider.host())];

    if let Some(err) = failure {
        failures().count(provider);
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
