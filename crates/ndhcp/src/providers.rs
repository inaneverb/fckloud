use {
    anyhow::{Context, Result},
    bytes::Bytes,
    derive_more::{Debug, Display},
    reqwest::{Method, header::HeaderMap},
    serde_json::from_slice as unjson,
    smallvec::SmallVec,
    std::{net::IpAddr, str::from_utf8_unchecked as b2s},
    strum::EnumCount,
    strum_macros::{
        AsRefStr, EnumCount, EnumIter, EnumString, IntoStaticStr, VariantArray, VariantNames,
    },
};

#[derive(
    Clone,
    Copy,
    Eq,
    PartialEq,
    Hash,
    Debug,
    Display,
    EnumIter,
    EnumCount,
    VariantArray,
    VariantNames,
    EnumString,
    IntoStaticStr,
    AsRefStr,
)]
pub enum HttpProvider {
    #[display("httpbin.org")]
    HttpBin,
}

pub type HttpProviders = SmallVec<[HttpProvider; HttpProvider::COUNT]>;

impl HttpProvider {
    pub const fn request_uri(&self) -> &'static str {
        match self {
            Self::HttpBin => "https://httpbin.org/ip",
        }
    }

    pub const fn request_method(&self) -> Method {
        match self {
            Self::HttpBin => Method::GET,
        }
    }

    pub fn response_decode(&self, headers: &HeaderMap, body: Bytes) -> Result<IpAddr> {
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
