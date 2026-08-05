use {
    crate::pubip::HttpProvider,
    std::{
        collections::HashMap,
        time::{Duration, Instant},
    },
};

/// What a round may ask for, once the providers still serving out their gap
/// have been set aside.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Split {
    pub allowed: Vec<HttpProvider>,
    pub holding: Vec<(HttpProvider, Duration)>,
}

/// Decides which providers may be asked, and how long the rest must wait.
///
/// Pure on purpose, like the consensus beside it: a gap that has or has not
/// elapsed is arithmetic, and a test should not need a clock to exercise it.
pub fn split(
    providers: &[HttpProvider],
    gaps: &HashMap<HttpProvider, Duration>,
    asked: &HashMap<HttpProvider, Instant>,
    now: Instant,
) -> Split {
    let mut result = Split::default();

    for provider in providers {
        match remaining(*provider, gaps, asked, now) {
            Some(left) => result.holding.push((*provider, left)),
            None => result.allowed.push(*provider),
        }
    }

    result
}

/// How long the provider still owes before it may be asked again, [`None`]
/// when it may be asked now.
fn remaining(
    provider: HttpProvider,
    gaps: &HashMap<HttpProvider, Duration>,
    asked: &HashMap<HttpProvider, Instant>,
    now: Instant,
) -> Option<Duration> {
    let gap = gap_of(provider, gaps)?;
    let last = asked.get(&provider)?;

    // `saturating_sub` and not a comparison: a clock that went backwards
    // should let a provider through, not hold it for the age of the universe.
    let elapsed = now.saturating_duration_since(*last);
    gap.checked_sub(elapsed).filter(|left| !left.is_zero())
}

/// The gap the provider asks for, or the one the operator put in its place.
pub fn gap_of(provider: HttpProvider, gaps: &HashMap<HttpProvider, Duration>) -> Option<Duration> {
    gaps.get(&provider)
        .copied()
        .or_else(|| provider.rate_limit())
        .filter(|gap| !gap.is_zero())
}

#[cfg(test)]
mod tests {
    use super::*;

    const LIMITED: HttpProvider = HttpProvider::MyIpWtf;
    const FREE: HttpProvider = HttpProvider::Ipify;

    fn at(now: Instant, ago: Duration) -> Instant {
        now.checked_sub(ago).expect("test instant must exist")
    }

    #[test]
    fn a_provider_never_asked_is_allowed() {
        let now = Instant::now();
        let out = split(&[LIMITED, FREE], &HashMap::new(), &HashMap::new(), now);

        assert_eq!(out.allowed, vec![LIMITED, FREE]);
        assert!(out.holding.is_empty());
    }

    #[test]
    fn a_provider_without_a_limit_is_always_allowed() {
        let now = Instant::now();
        let asked = HashMap::from([(FREE, now)]);
        let out = split(&[FREE], &HashMap::new(), &asked, now);

        assert_eq!(out.allowed, vec![FREE]);
    }

    #[test]
    fn a_limited_provider_holds_until_its_gap_elapses() {
        let now = Instant::now();
        let asked = HashMap::from([(LIMITED, at(now, Duration::from_secs(20)))]);
        let out = split(&[LIMITED], &HashMap::new(), &asked, now);

        assert!(out.allowed.is_empty());
        assert_eq!(out.holding, vec![(LIMITED, Duration::from_secs(40))]);
    }

    #[test]
    fn a_limited_provider_is_allowed_once_its_gap_has_passed() {
        let now = Instant::now();
        let asked = HashMap::from([(LIMITED, at(now, Duration::from_secs(61)))]);
        let out = split(&[LIMITED], &HashMap::new(), &asked, now);

        assert_eq!(out.allowed, vec![LIMITED]);
    }

    #[test]
    fn an_override_replaces_the_published_gap() {
        let now = Instant::now();
        let gaps = HashMap::from([(LIMITED, Duration::from_secs(5))]);
        let asked = HashMap::from([(LIMITED, at(now, Duration::from_secs(10)))]);

        assert_eq!(split(&[LIMITED], &gaps, &asked, now).allowed, vec![LIMITED]);
    }

    #[test]
    fn an_override_can_add_a_gap_where_the_provider_asked_for_none() {
        let now = Instant::now();
        let gaps = HashMap::from([(FREE, Duration::from_mins(5))]);
        let asked = HashMap::from([(FREE, now)]);

        assert!(split(&[FREE], &gaps, &asked, now).allowed.is_empty());
    }

    #[test]
    fn a_zero_override_lifts_the_gap_entirely() {
        let now = Instant::now();
        let gaps = HashMap::from([(LIMITED, Duration::ZERO)]);
        let asked = HashMap::from([(LIMITED, now)]);

        assert_eq!(split(&[LIMITED], &gaps, &asked, now).allowed, vec![LIMITED]);
    }
}
