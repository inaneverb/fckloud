use {
    anyhow::{Context, Result},
    bytes::Bytes,
    derive_more::{Debug, Display},
    reqwest::{Method, header::HeaderMap},
    serde_json::from_slice as unjson,
    std::{net::IpAddr, str::from_utf8_unchecked as b2s},
    strum_macros::{EnumCount, EnumIter, EnumString, VariantArray},
};

#[derive(Clone, Copy, Eq, PartialEq, Hash, Debug, Display, EnumIter, EnumCount, VariantArray, EnumString)]
pub enum HttpProvider {
    #[display("httpbin.org")]
    HttpBin,
}

impl HttpProvider {
    pub(crate) const fn request_uri(&self) -> &'static str {
        match self {
            Self::HttpBin => "https://httpbin.org/ip",
        }
    }

    pub(crate) const fn request_method(&self) -> Method {
        match self {
            Self::HttpBin => Method::GET,
        }
    }

    pub(crate) fn response_decode(&self, headers: &HeaderMap, body: Bytes) -> Result<IpAddr> {
        match self {
            Self::HttpBin => decode_httpbin(headers, body),
        }
    }
}

fn decode_httpbin(_: &HeaderMap, body: Bytes) -> Result<IpAddr> {
    #[derive(serde::Deserialize)]
    struct ResponseTyped {
        origin: IpAddr,
    }

    let resp_typed: ResponseTyped = unjson(&body)
        .with_context(|| unsafe { format!("cannot decode HTTP response, data: {}", b2s(&body)) })?;

    Ok(resp_typed.origin)
}
