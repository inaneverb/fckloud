use {crate::pubip::HttpProvider, std::collections::HashMap};

/// The mutable source of trust factors for every known [`HttpProvider`], and
/// the arithmetic that turns them into the threshold an address must reach.
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
    pub fn set_trust_factor(&mut self, provider: HttpProvider, new_trust_factor: usize) {
        assert!(Self::is_valid(new_trust_factor));
        self.custom.insert(provider, new_trust_factor);
    }

    /// Calculates and returns the **confirmation number** that must be achieved
    /// by every IP to consider it confirmed.
    ///
    /// Two thirds of the total trust, rounded up while only two providers are
    /// enabled and rounded down once there are three or more, so the threshold
    /// stays reachable without demanding unanimity.
    pub fn calc_confirmation_number(&self, providers: &[HttpProvider]) -> usize {
        const NUMERATOR: usize = 2;
        const DENOMINATOR: usize = 3;

        let total: usize = providers
            .iter()
            .map(|provider| self.trust_factor(*provider))
            .sum();

        match providers.len() {
            0 => unreachable!("confirmation number is undefined when no providers are given"),
            1 => total,
            2 => (total * NUMERATOR).div_ceil(DENOMINATOR),
            3.. => total * NUMERATOR / DENOMINATOR,
        }
    }

    // Returns default trust factor for the given [HttpProvider].
    fn default_trust_factor(provider: HttpProvider) -> usize {
        match provider {
            HttpProvider::HttpBin => Self::LOW,
            HttpProvider::MyIpWtf | HttpProvider::SeeIp | HttpProvider::MyIpCom => Self::MED,
            HttpProvider::Ipify => Self::HIG,
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

        // Every provider there is, total 10: floor(20/3) = 6.
        assert_eq!(tfa.calc_confirmation_number(HttpProvider::VARIANTS), 6);
    }

    #[test]
    fn confirmation_number_rounds_up_at_two_providers_and_down_beyond() {
        let mut tfa = TrustFactorAuthority::default();
        tfa.set_trust_factor(HttpProvider::HttpBin, TrustFactorAuthority::MED);

        // Two providers, total 4: ceil(8/3) = 3, so neither confirms alone.
        let two = [HttpProvider::HttpBin, HttpProvider::MyIpWtf];
        assert_eq!(tfa.calc_confirmation_number(&two), 3);

        // Three providers, total 6: floor(12/3) = 4, so two of three suffice.
        let three = [
            HttpProvider::HttpBin,
            HttpProvider::MyIpWtf,
            HttpProvider::MyIpWtf,
        ];
        assert_eq!(tfa.calc_confirmation_number(&three), 4);
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
