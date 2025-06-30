use {
    anyhow::{Context, Result, bail},
    k8s_openapi::{api::core::v1::Node, apimachinery::pkg::version::Info as ApiVersionInfo},
    kube::{
        Api, Client, Config, Resource,
        api::{self, ObjectMeta},
    },
    std::{
        fmt::{Debug, Display},
        time::Duration,
    },
    tracing::{error, info, instrument, warn},
};

pub struct Manager {
    api_nodes: Api<Node>,
    node_name: String,
}

impl Manager {
    // Creates and returns a [Manager] that connects to the Kubernetes cluster.
    // Verifies the connection, ensures the given `node_name` exists,
    // and that the Nodes API is accessible.
    // Returns an error if any check fails. Panics if `node_name` is empty.
    #[instrument(name = "kube_connect")]
    pub async fn new<S>(node_name: S) -> Result<Self>
    where
        S: ToString + Display + Debug,
    {
        let node_name = node_name.to_string();
        assert!(!node_name.is_empty());

        // Let's build the Kubernetes client.
        // We want to "ping" it aswell, so use version check for it.

        let config = Self::get_config().await?;
        let client = Client::try_from(config.clone())?;

        client
            .apiserver_version()
            .await
            .and_then(|ver| Ok(format!("{}.{}", ver.major, ver.minor)))
            .and_then(|ver| Ok((ver, config.cluster_url.host())))
            .inspect(|(apiserver_version, apiserver_host)| {
                info!(
                    apiserver_version,
                    apiserver_host, "connected to the Kubernetes cluster"
                )
            })?;

        // Validate, that nodes API is accessible and that node does exist.

        let api_nodes: Api<Node> = Api::all(client.clone());
        api_nodes
            .get(&node_name)
            .await
            .with_context(|| format!("cannot query the requested Node"))
        ?;

        Ok(Self {
            api_nodes,
            node_name,
        })
    }

    // Helper to get the Kubernetes config, with some defaults possibly overridden.
    async fn get_config() -> Result<Config> {
        const CONNECTION_TIMEOUT: Duration = Duration::from_secs(5);

        let mut config = Config::infer().await?;
        config.connect_timeout = CONNECTION_TIMEOUT.into();
        Ok(config)
    }
}
