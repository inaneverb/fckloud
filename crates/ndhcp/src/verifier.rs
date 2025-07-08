use crate::providers::HttpProvider;
use anyhow::Result;
use reqwest::Client;
use std::net::IpAddr;

/// Performs HTTP request to
pub async fn get_public_ip(prov: HttpProvider) -> Result<IpAddr> {
    let client = Client::builder().build()?;

    let response = client
        .request(prov.request_method(), prov.request_uri())
        .send()
        .await?;

    let headers = response.headers().clone();
    let body = response.bytes().await?;

    prov.response_decode(&headers, body)
}
