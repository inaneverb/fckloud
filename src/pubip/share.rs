use std::{fmt, str::FromStr};

/// The part of the enabled trust an address must gather to be confirmed.
///
/// Kept as an exact ratio and never as a float: the threshold has always been
/// two thirds rather than an approximation of it, and `0.67` demanding more
/// agreement than documented is a bug this project has already shipped once.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct TrustShare {
    numerator: usize,
    denominator: usize,
}

impl TrustShare {
    pub const DEFAULT: Self = Self {
        numerator: 2,
        denominator: 3,
    };

    /// The share of the given total, rounded down.
    pub fn floor_of(self, total: usize) -> usize {
        total.saturating_mul(self.numerator) / self.denominator
    }

    /// The share of the given total, rounded up.
    pub fn ceil_of(self, total: usize) -> usize {
        total
            .saturating_mul(self.numerator)
            .div_ceil(self.denominator)
    }

    /// Whether the share can ask for something an operator could actually give:
    /// nothing above the whole trust, and nothing at or below none of it.
    pub fn is_valid(self) -> bool {
        self.numerator > 0 && self.numerator <= self.denominator
    }
}

impl Default for TrustShare {
    fn default() -> Self {
        Self::DEFAULT
    }
}

impl fmt::Display for TrustShare {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}/{}", self.numerator, self.denominator)
    }
}

impl FromStr for TrustShare {
    type Err = String;

    /// Accepts `2/3`, `75%` and `0.667`, all of which become exact ratios.
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let s = s.trim();

        if s.is_empty() {
            return Err("a share needs a value, such as 2/3, 75% or 0.75".into());
        }

        let share = if let Some((num, den)) = s.split_once('/') {
            Self {
                numerator: parse_number(num)?,
                denominator: parse_number(den)?,
            }
        } else if let Some(percent) = s.strip_suffix('%') {
            decimal(percent.trim(), 100)?
        } else {
            decimal(s, 1)?
        };

        if share.denominator == 0 {
            return Err("a share cannot be divided by zero".into());
        }

        Ok(share.reduced())
    }
}

impl TrustShare {
    fn reduced(self) -> Self {
        let divisor = gcd(self.numerator, self.denominator).max(1);
        Self {
            numerator: self.numerator / divisor,
            denominator: self.denominator / divisor,
        }
    }
}

/// Turns `0.667` into 667/1000, scaling the denominator by `over` so that a
/// percentage is the same code path as a plain decimal.
fn decimal(s: &str, over: usize) -> Result<TrustShare, String> {
    const TOO_MANY: &str = "that is a lot of decimal places";

    if s.is_empty() {
        return Err("a share needs a number".into());
    }

    let (whole, fraction) = s.split_once('.').unwrap_or((s, ""));

    let places = u32::try_from(fraction.len()).map_err(|_| TOO_MANY)?;
    let scale = 10usize.checked_pow(places).ok_or(TOO_MANY)?;

    let whole = if whole.is_empty() {
        0
    } else {
        parse_number(whole)?
    };
    let fraction = if fraction.is_empty() {
        0
    } else {
        parse_number(fraction)?
    };

    Ok(TrustShare {
        numerator: whole.saturating_mul(scale).saturating_add(fraction),
        denominator: scale.saturating_mul(over),
    })
}

fn parse_number(s: &str) -> Result<usize, String> {
    s.trim()
        .parse()
        .map_err(|_| format!("`{s}` is not a whole number"))
}

const fn gcd(a: usize, b: usize) -> usize {
    if b == 0 { a } else { gcd(b, a % b) }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn share(s: &str) -> TrustShare {
        TrustShare::from_str(s).unwrap_or_else(|err| panic!("`{s}` must parse: {err}"))
    }

    #[test]
    fn the_three_spellings_of_two_thirds_agree() {
        assert_eq!(share("2/3"), TrustShare::DEFAULT);
        assert_eq!(share("4/6"), TrustShare::DEFAULT);

        // Not two thirds, and deliberately not pretending to be.
        assert_ne!(share("0.667"), TrustShare::DEFAULT);
    }

    #[test]
    fn a_percentage_is_the_ratio_it_names() {
        assert_eq!(share("75%"), share("3/4"));
        assert_eq!(share("50%"), share("1/2"));
        assert_eq!(share("100%"), share("1/1"));
        assert_eq!(share("66.5%"), share("133/200"));
    }

    #[test]
    fn a_decimal_is_the_ratio_it_names() {
        assert_eq!(share("0.5"), share("1/2"));
        assert_eq!(share("0.25"), share("1/4"));
        assert_eq!(share("1"), share("1/1"));
    }

    #[test]
    fn two_thirds_of_eleven_is_seven_rounded_down_and_eight_rounded_up() {
        assert_eq!(TrustShare::DEFAULT.floor_of(11), 7);
        assert_eq!(TrustShare::DEFAULT.ceil_of(11), 8);
    }

    #[test]
    fn the_exact_ratio_does_not_drift_the_way_a_decimal_would() {
        // 0.67 of 3 rounds up to 3 and demands unanimity; two thirds does not.
        assert_eq!(TrustShare::DEFAULT.ceil_of(3), 2);
        assert_eq!(share("0.67").ceil_of(3), 3);
    }

    #[test]
    fn a_share_of_none_or_more_than_all_is_rejected() {
        assert!(TrustShare::DEFAULT.is_valid());
        assert!(share("100%").is_valid());

        assert!(!share("0").is_valid());
        assert!(!share("0%").is_valid());
        assert!(!share("4/3").is_valid());
        assert!(!share("150%").is_valid());
    }

    #[test]
    fn nonsense_does_not_parse() {
        for bad in ["", "two thirds", "2/", "/3", "2/0", "%", "-1"] {
            assert!(TrustShare::from_str(bad).is_err(), "`{bad}` must not parse");
        }
    }

    #[test]
    fn a_share_prints_as_the_ratio_it_is() {
        assert_eq!(TrustShare::DEFAULT.to_string(), "2/3");
        assert_eq!(share("75%").to_string(), "3/4");
    }
}
