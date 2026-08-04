use {
    anyhow::{Context, Result},
    bytes::Bytes,
    derive_more::{Debug, Display},
    reqwest::Method,
    serde::Deserialize,
    serde_json::from_slice as unjson,
    smallvec::SmallVec,
    std::net::IpAddr,
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
    #[display("myip.wtf")]
    MyIpWtf, // https://myip.wtf/automation
}

pub type HttpProviders = SmallVec<[HttpProvider; HttpProvider::COUNT]>;

impl HttpProvider {
    pub const fn request_uri(&self) -> &'static str {
        match self {
            Self::HttpBin => "https://httpbin.org/ip",
            Self::MyIpWtf => "https://myip.wtf/json",
        }
    }

    pub const fn request_method(&self) -> Method {
        match self {
            Self::HttpBin | Self::MyIpWtf => Method::GET,
        }
    }

    pub fn response_decode(&self, body: &Bytes) -> Result<IpAddr> {
        match self {
            Self::HttpBin => decode_json::<HttpBinResponse>(body),
            Self::MyIpWtf => decode_json::<MyIpWtfResponse>(body),
        }
    }
}

/// What every provider's response boils down to, whatever it calls the field.
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

fn decode_json<T: Response>(body: &[u8]) -> Result<IpAddr> {
    let decoded: T = unjson(body).with_context(|| {
        format!(
            "cannot decode HTTP response, data: {}",
            String::from_utf8_lossy(body)
        )
    })?;

    Ok(decoded.into_ip_addr())
}
