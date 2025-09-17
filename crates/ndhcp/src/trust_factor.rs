use {
    crate::providers::{HttpProvider},
    std::collections::HashMap,
};

/// Represents the mutable source of trust factors for the every known [HttpProvider].
///
/// Also provides a way to calculate confirmation number that must be achieved
/// to consider some IP confirmed. Read more: [Self::calc_confirmation_number].
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
        match trust_factor {
            Self::LOW | Self::MED | Self::HIG => true,
            _ => false,
        }
    }

    /// Returns trust factor for the given [HttpProvider] that is
    /// either defined by the user via [Self::set_trust_factor] or default one.
    pub fn trust_factor(&self, provider: HttpProvider) -> usize {
        self.custom
            .get(&provider)
            .cloned()
            .unwrap_or(Self::default_trust_factor(provider))
    }

    /// Overwrites default trust factor for the given [HttpProvider].
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
    pub fn calc_confirmation_number(&self, providers: &[HttpProvider]) -> usize {
        const COMPENSATION_FACTOR: f32 = 0.67; // 2/3

        let trust_factor_total = providers
            .iter()
            .map(|provider| self.trust_factor(*provider))
            .sum();

        match providers.len() {
            0 => unreachable!("confirmation number is undefined when no providers are given"),
            1 => trust_factor_total,
            2 => (trust_factor_total as f32 * COMPENSATION_FACTOR).ceil() as usize,
            3.. => (trust_factor_total as f32 * COMPENSATION_FACTOR).floor() as usize,
        }
    }

    // Returns default trust factor for the given [HttpProvider].
    fn default_trust_factor(provider: HttpProvider) -> usize {
        match provider {
            HttpProvider::HttpBin => Self::LOW,
        }
    }
}
