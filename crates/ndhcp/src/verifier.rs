use {
    std::{
        net::IpAddr,
        result::Result as StdResult,
    },

    anyhow::{anyhow, Error, Result},
    derive_more::{Debug, Display},
    reqwest::Client,

    crate::{
        address,
        providers::HttpProvider,
    },
};

#[derive(Debug, Display)]
pub enum Reason {
    InappropriateAddress(address::Kind),
    RemoteUnmatched(IpAddr),
    RemoteError(Error),
}

/// Performs HTTP request to
pub async fn get_public_ip(prov: HttpProvider, local_ip: IpAddr) -> Result<IpAddr> {

    let client = Client::builder()
        .local_address(local_ip)
        .build()?;

    let response = client
        .request(prov.request_method(), prov.request_uri())
        .send()
        .await?;

    let headers = response.headers().clone();
    let body = response.bytes().await?;

    prov.response_decode(&headers, body)
}

pub async fn check_public_ip(prov: HttpProvider, local_ip: IpAddr) -> StdResult<(), Reason> {
    let remote_ip = get_public_ip(prov, local_ip)
        .await
        .or_else(|err| Err(Reason::RemoteError(anyhow!(err))))?;

    if remote_ip != local_ip {
        return Err(Reason::RemoteUnmatched(remote_ip))
    }

    Ok(())
}

