use {
    reqwest::StatusCode,
    std::{error::Error, fmt, net::IpAddr},
};

/// Why a provider's answer could not be used this round.
///
/// The variants are a closed taxonomy on purpose: [`Self::as_error_type`] is
/// what a telemetry label is built from, and a label built from an error
/// message mints a new time series per message.
#[derive(Debug)]
pub enum FetchError {
    Timeout,
    Unreachable(reqwest::Error),
    HttpStatus(StatusCode),
    Decode {
        body: String,
        source: serde_json::Error,
    },
    NotPublic(IpAddr),
}

impl FetchError {
    /// The `error.type` attribute value. Static strings, never the detail:
    /// the status code and the offending body belong in the log, not here.
    pub const fn as_error_type(&self) -> &'static str {
        match self {
            Self::Timeout => "timeout",
            Self::Unreachable(_) => "unreachable",
            Self::HttpStatus(_) => "http_status",
            Self::Decode { .. } => "decode",
            Self::NotPublic(_) => "not_public",
        }
    }

    /// The status the provider answered with, if it answered at all. Kept
    /// apart from [`Self::as_error_type`] so that a 429 stays distinguishable
    /// from a 503 without either becoming an error kind of its own.
    pub fn status_code(&self) -> Option<u16> {
        match self {
            Self::HttpStatus(status) => Some(status.as_u16()),
            _ => None,
        }
    }
}

impl fmt::Display for FetchError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Timeout => f.write_str("provider timed out"),
            Self::Unreachable(err) => write!(f, "provider is unreachable: {err}"),
            Self::HttpStatus(status) => write!(f, "provider responded with {status}"),
            Self::Decode { body, source } => {
                write!(f, "cannot decode the response: {source}, data: {body}")
            }
            Self::NotPublic(ip_addr) => {
                write!(f, "provider reported a non-public address {ip_addr}")
            }
        }
    }
}

impl Error for FetchError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Unreachable(err) => Some(err),
            Self::Decode { source, .. } => Some(source),
            _ => None,
        }
    }
}

impl From<reqwest::Error> for FetchError {
    fn from(err: reqwest::Error) -> Self {
        if err.is_timeout() {
            Self::Timeout
        } else {
            Self::Unreachable(err)
        }
    }
}

#[cfg(test)]
mod tests {
    use {super::*, std::str::FromStr};

    #[test]
    fn every_variant_has_its_own_error_type() {
        let types = [
            FetchError::Timeout.as_error_type(),
            FetchError::HttpStatus(StatusCode::TOO_MANY_REQUESTS).as_error_type(),
            FetchError::NotPublic(IpAddr::from_str("10.0.0.1").expect("test address must parse"))
                .as_error_type(),
        ];

        assert_eq!(types, ["timeout", "http_status", "not_public"]);
    }

    #[test]
    fn a_non_public_address_is_named_in_the_message() {
        let err = FetchError::NotPublic(IpAddr::from_str("10.0.0.1").expect("must parse"));
        assert!(err.to_string().contains("10.0.0.1"));
    }
}
