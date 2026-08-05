use {
    crate::pubip::error::FetchError,
    reqwest::Method,
    serde::Deserialize,
    serde_json::from_slice as unjson,
    std::{fmt, net::IpAddr},
    strum::{EnumString, VariantArray, VariantNames},
};

#[derive(Clone, Copy, Eq, PartialEq, Hash, Debug, EnumString, VariantArray, VariantNames)]
pub enum HttpProvider {
    HttpBin,
    MyIpWtf,      // https://myip.wtf/automation
    SeeIp,        // https://seeip.org
    Ipify,        // https://www.ipify.org
    MyIpCom,      // https://www.myip.com/api-docs
    BigDataCloud, // https://www.bigdatacloud.com/free-api/public-ip-address-api
    MyIpLa,       // https://www.myip.la
}

impl fmt::Display for HttpProvider {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.host())
    }
}

impl HttpProvider {
    /// The provider's host. Borrowed rather than formatted, so that using it
    /// as a telemetry attribute costs no allocation per request.
    pub const fn host(self) -> &'static str {
        match self {
            Self::HttpBin => "httpbin.org",
            Self::MyIpWtf => "myip.wtf",
            Self::SeeIp => "api.seeip.org",
            Self::Ipify => "api64.ipify.org",
            Self::MyIpCom => "api.myip.com",
            Self::BigDataCloud => "api.bigdatacloud.net",
            Self::MyIpLa => "api.myip.la",
        }
    }

    pub const fn request_uri(self) -> &'static str {
        match self {
            Self::HttpBin => "https://httpbin.org/ip",
            Self::MyIpWtf => "https://myip.wtf/json",
            Self::SeeIp => "https://api.seeip.org/jsonip",
            // `api64` answers over whichever family the connection arrived on.
            // `api.ipify.org` publishes no AAAA, which would pin this provider
            // to IPv4 while dual-stacked providers report the node's IPv6, and
            // a round split between two families confirms neither address.
            Self::Ipify => "https://api64.ipify.org/?format=json",
            Self::MyIpCom => "https://api.myip.com/",
            Self::BigDataCloud => "https://api.bigdatacloud.net/data/client-ip",
            Self::MyIpLa => "https://api.myip.la/en?json",
        }
    }

    pub const fn request_method(self) -> Method {
        match self {
            Self::HttpBin
            | Self::MyIpWtf
            | Self::SeeIp
            | Self::Ipify
            | Self::MyIpCom
            | Self::BigDataCloud
            | Self::MyIpLa => Method::GET,
        }
    }

    /// Whether the provider takes part in consensus without being asked for.
    pub const fn enabled_by_default(self) -> bool {
        match self {
            Self::HttpBin
            | Self::MyIpWtf
            | Self::SeeIp
            | Self::Ipify
            | Self::MyIpCom
            | Self::BigDataCloud
            | Self::MyIpLa => true,
        }
    }

    pub fn response_decode(self, body: &[u8]) -> Result<IpAddr, FetchError> {
        match self {
            Self::HttpBin => decode::<HttpBinResponse>(body),
            Self::MyIpWtf => decode::<MyIpWtfResponse>(body),
            Self::BigDataCloud => decode::<BigDataCloudResponse>(body),
            Self::SeeIp | Self::Ipify | Self::MyIpCom | Self::MyIpLa => {
                decode::<IpFieldResponse>(body)
            }
        }
    }
}

/// What every provider's answer boils down to, whatever it calls the field.
trait Response: for<'de> Deserialize<'de> {
    fn into_ip_addr(self) -> IpAddr;
}

#[derive(Deserialize)]
struct HttpBinResponse {
    origin: IpAddr,
}

#[derive(Deserialize)]
struct MyIpWtfResponse {
    #[serde(rename = "YourFuckingIPAddress")]
    ip_addr: IpAddr,
}

/// The plainest shape there is: an `ip` field, and whatever geography the
/// provider decided to put beside it.
#[derive(Deserialize)]
struct IpFieldResponse {
    ip: IpAddr,
}

// The published example names this field `ip`; what the endpoint sends is
// `ipString`. Trusting the documentation here decodes nothing.
#[derive(Deserialize)]
struct BigDataCloudResponse {
    #[serde(rename = "ipString")]
    ip_addr: IpAddr,
}

impl Response for HttpBinResponse {
    fn into_ip_addr(self) -> IpAddr {
        self.origin
    }
}

impl Response for MyIpWtfResponse {
    fn into_ip_addr(self) -> IpAddr {
        self.ip_addr
    }
}

impl Response for IpFieldResponse {
    fn into_ip_addr(self) -> IpAddr {
        self.ip
    }
}

impl Response for BigDataCloudResponse {
    fn into_ip_addr(self) -> IpAddr {
        self.ip_addr
    }
}

fn decode<T: Response>(body: &[u8]) -> Result<IpAddr, FetchError> {
    let decoded: T = unjson(body).map_err(|source| FetchError::Decode {
        body: String::from_utf8_lossy(body).into_owned(),
        source,
    })?;

    Ok(decoded.into_ip_addr())
}

#[cfg(test)]
mod tests {
    use super::*;

    // One captured body per provider, trimmed of everything but the fields
    // that matter, so that a provider changing its shape fails here first.
    const SHAPES: [(HttpProvider, &str); 7] = [
        (HttpProvider::HttpBin, r#"{"origin":"1.2.3.4"}"#),
        (
            HttpProvider::MyIpWtf,
            r#"{"YourFuckingIPAddress":"1.2.3.4"}"#,
        ),
        (HttpProvider::SeeIp, r#"{"ip":"1.2.3.4"}"#),
        (HttpProvider::Ipify, r#"{"ip":"1.2.3.4"}"#),
        (
            HttpProvider::MyIpCom,
            r#"{"ip":"1.2.3.4","country":"Serbia","cc":"RS"}"#,
        ),
        (
            HttpProvider::BigDataCloud,
            r#"{"ipString":"1.2.3.4","ipType":"IPv4"}"#,
        ),
        (
            HttpProvider::MyIpLa,
            r#"{"ip":"1.2.3.4","location":{"country_code":"RS"}}"#,
        ),
    ];

    #[test]
    fn each_provider_decodes_its_own_shape() {
        for (provider, body) in SHAPES {
            let decoded = provider
                .response_decode(body.as_bytes())
                .unwrap_or_else(|err| panic!("{provider} shape must decode: {err}"));

            assert_eq!(decoded.to_string(), "1.2.3.4", "{provider} decoded wrongly");
        }
    }

    #[test]
    fn every_provider_has_a_captured_shape() {
        for provider in <HttpProvider as VariantArray>::VARIANTS {
            assert!(
                SHAPES.iter().any(|(covered, _)| covered == provider),
                "{provider} has no captured response body to decode",
            );
        }
    }

    #[test]
    fn the_uri_of_every_provider_is_https_and_names_its_host() {
        for provider in <HttpProvider as VariantArray>::VARIANTS {
            let uri = provider.request_uri();
            let expected = format!("https://{}/", provider.host());

            assert!(
                uri.starts_with(&expected),
                "{uri} does not start {expected}"
            );
        }
    }

    #[test]
    fn the_bigdatacloud_documented_field_name_is_not_the_one_it_sends() {
        HttpProvider::BigDataCloud
            .response_decode(br#"{"ip":"1.2.3.4"}"#)
            .expect_err("`ip` is what the docs show, not what the endpoint sends");
    }

    #[test]
    fn a_body_that_is_not_json_reports_what_it_saw() {
        let err = HttpProvider::HttpBin
            .response_decode(b"<html>502 Bad Gateway</html>")
            .expect_err("HTML must not decode as an address");

        assert_eq!(err.as_error_type(), "decode");
        assert!(err.to_string().contains("502 Bad Gateway"));
    }

    #[test]
    fn invalid_utf8_does_not_panic() {
        let err = HttpProvider::MyIpWtf
            .response_decode(&[0xff, 0xfe, 0x00, 0x80])
            .expect_err("garbage must not decode as an address");

        assert!(!err.to_string().is_empty());
    }
}
