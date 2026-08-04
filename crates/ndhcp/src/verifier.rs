use {
    crate::{address, providers::HttpProvider},
    anyhow::{Context, Result, ensure},
    reqwest::Client,
    std::{net::IpAddr, sync::LazyLock, time::Duration},
};

const USER_AGENT: &str = concat!("fckloud/", env!("CARGO_PKG_VERSION"));

const CONNECT_TIMEOUT: Duration = Duration::from_secs(5);
const REQUEST_TIMEOUT: Duration = Duration::from_secs(15);

// One client, one connection pool, one place where a stalled provider dies.
static CLIENT: LazyLock<Client> = LazyLock::new(|| {
    Client::builder()
        .user_agent(USER_AGENT)
        .connect_timeout(CONNECT_TIMEOUT)
        .timeout(REQUEST_TIMEOUT)
        .build()
        .expect("HTTP client with static settings must be constructible")
});

/// Asks the given [`HttpProvider`] which public IP address it sees us as.
pub async fn get_public_ip(provider: HttpProvider) -> Result<IpAddr> {
    let response = CLIENT
        .request(provider.request_method(), provider.request_uri())
        .send()
        .await
        .context("provider is unreachable")?
        .error_for_status()
        .context("provider responded with an error")?;

    let body = response.bytes().await.context("cannot read the response")?;
    let ip_addr = provider.response_decode(&body)?;

    // A node's ExternalIP that is not routable on the Internet is a lie,
    // no matter how confidently a provider states it.
    ensure!(
        address::is_public(&ip_addr),
        "provider reported a non-public address {} ({})",
        ip_addr,
        address::kind(&ip_addr)
    );

    Ok(ip_addr)
}
