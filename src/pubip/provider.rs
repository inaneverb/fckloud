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
    MyIpWtf, // https://myip.wtf/automation
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
        }
    }

    pub const fn request_uri(self) -> &'static str {
        match self {
            Self::HttpBin => "https://httpbin.org/ip",
            Self::MyIpWtf => "https://myip.wtf/json",
        }
    }

    pub const fn request_method(self) -> Method {
        match self {
            Self::HttpBin | Self::MyIpWtf => Method::GET,
        }
    }

    /// Whether the provider takes part in consensus without being asked for.
    pub const fn enabled_by_default(self) -> bool {
        match self {
            Self::HttpBin | Self::MyIpWtf => true,
        }
    }

    pub fn response_decode(self, body: &[u8]) -> Result<IpAddr, FetchError> {
        match self {
            Self::HttpBin => decode::<HttpBinResponse>(body),
            Self::MyIpWtf => decode::<MyIpWtfResponse>(body),
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

    #[test]
    fn each_provider_decodes_its_own_shape() {
        let httpbin = HttpProvider::HttpBin
            .response_decode(br#"{"origin": "1.2.3.4"}"#)
            .expect("httpbin shape must decode");
        assert_eq!(httpbin.to_string(), "1.2.3.4");

        let myipwtf = HttpProvider::MyIpWtf
            .response_decode(br#"{"YourFuckingIPAddress": "5.6.7.8"}"#)
            .expect("myip.wtf shape must decode");
        assert_eq!(myipwtf.to_string(), "5.6.7.8");
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
