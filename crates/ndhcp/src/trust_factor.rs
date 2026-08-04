use {crate::providers::HttpProvider, std::collections::HashMap};

/// Represents the mutable source of trust factors for the every known [`HttpProvider`].
///
/// Also provides a way to calculate confirmation number that must be achieved
/// to consider some IP confirmed. Read more: [`Self::calc_confirmation_number`].
#[derive(Default)]
pub struct TrustFactorAuthority {
    custom: HashMap<HttpProvider, usize>,
}

impl TrustFactorAuthority {
    pub const LOW: usize = 1;
    pub const MED: usize = 2;
    pub const HIG: usize = 3;

    /// Reports whether given trust factor is valid or not.
    pub fn is_valid(trust_factor: usize) -> bool {
        (Self::LOW..=Self::HIG).contains(&trust_factor)
    }

    /// Returns trust factor for the given [`HttpProvider`] that is
    /// either defined by the user via [`Self::set_trust_factor`] or default one.
    pub fn trust_factor(&self, provider: HttpProvider) -> usize {
        self.custom
            .get(&provider)
            .copied()
            .unwrap_or_else(|| Self::default_trust_factor(provider))
    }

    /// Overwrites default trust factor for the given [`HttpProvider`].
    /// New trust factor must be in valid range, panic otherwise.
    pub fn set_trust_factor(&mut self, provider: &HttpProvider, new_trust_factor: usize) {
        assert!(Self::is_valid(new_trust_factor));
        self.custom.insert(*provider, new_trust_factor);
    }

    /// Calculates and returns the **confirmation number** that must be achieved
    /// by every IP to consider it confirmed.
    ///
    /// During the process of verification, each provider's trust factor
    /// that reported the same IP is added to that IP's confirmation's bucket.
    ///
    /// When that bucket reaches the confirmation number that is returned
    /// by this func or re-defined by the user, the IP is considered confirmed.
    ///
    /// The threshold is two thirds of the total trust, rounded up while the
    /// providers are few enough that one of them going dark must not decide
    /// alone, and rounded down once there are enough of them that a strict
    /// two thirds would be unreachable in practice.
    pub fn calc_confirmation_number(&self, providers: &[HttpProvider]) -> usize {
        const NUMERATOR: usize = 2;
        const DENOMINATOR: usize = 3;

        let trust_factor_total: usize = providers
            .iter()
            .map(|provider| self.trust_factor(*provider))
            .sum();

        match providers.len() {
            0 => unreachable!("confirmation number is undefined when no providers are given"),
            1 => trust_factor_total,
            2 => (trust_factor_total * NUMERATOR).div_ceil(DENOMINATOR),
            3.. => trust_factor_total * NUMERATOR / DENOMINATOR,
        }
    }

    // Returns default trust factor for the given [HttpProvider].
    fn default_trust_factor(provider: HttpProvider) -> usize {
        match provider {
            HttpProvider::HttpBin => Self::LOW,
            HttpProvider::MyIpWtf => Self::MED,
        }
    }
}

#[cfg(test)]
mod tests {
    use {super::*, strum::VariantArray};

    #[test]
    fn confirmation_number_matches_two_thirds_of_total_trust() {
        let tfa = TrustFactorAuthority::default();

        assert_eq!(tfa.calc_confirmation_number(&[HttpProvider::HttpBin]), 1);
        assert_eq!(tfa.calc_confirmation_number(&[HttpProvider::MyIpWtf]), 2);
        assert_eq!(tfa.calc_confirmation_number(HttpProvider::VARIANTS), 2);
    }

    #[test]
    fn every_default_trust_factor_is_valid() {
        for provider in HttpProvider::VARIANTS {
            assert!(TrustFactorAuthority::is_valid(
                TrustFactorAuthority::default_trust_factor(*provider)
            ));
        }
    }
}
