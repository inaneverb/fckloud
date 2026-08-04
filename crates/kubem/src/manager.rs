use {
    anyhow::{Context, Result, bail},
    k8s_openapi::api::core::v1::{Node, NodeAddress},
    kube::{
        Api, Client, Config,
        api::{Patch, PatchParams},
    },
    serde_json::json,
    std::{
        collections::{BTreeMap, BTreeSet},
        fmt::{Debug, Display},
        mem,
        net::IpAddr,
        str::FromStr,
        time::Duration,
    },
    strum_macros::EnumIs,
    tracing::{debug, warn},
};

pub struct Manager {
    api_nodes: Api<Node>,
    node_name: String,

    dry_run: bool,
    remove_unstaged: bool,

    pending: BTreeSet<IpAddr>,
    previous: BTreeSet<IpAddr>,
}

#[derive(EnumIs)]
pub enum AddrStatus {
    New,
    Skipped,
    Removed,
}

impl Manager {
    const TYPE_EXTERNAL_IP: &'static str = "ExternalIP";
    const CONNECTION_TIMEOUT: Duration = Duration::from_secs(5);

    /// Creates and returns a [Manager] that connects to the Kubernetes cluster.
    /// Verifies the connection, ensures the given `node_name` exists,
    /// and that the Nodes API is accessible.
    /// Returns an error if any check fails. Panics if `node_name` is empty.
    pub async fn new<S>(node_name: S) -> Result<Self>
    where
        S: ToString + Display + Debug,
    {
        let node_name = node_name.to_string();
        assert!(!node_name.is_empty());

        let mut config = Config::infer().await?;
        config.connect_timeout = Self::CONNECTION_TIMEOUT.into();

        let cluster_host = config.cluster_url.host().map(ToString::to_string);
        let client = Client::try_from(config)?;

        let version = client.apiserver_version().await?;
        debug!(
            version = format!("{}.{}", version.major, version.minor),
            host = cluster_host,
            "connected to the Kubernetes cluster"
        );

        let manager = Self {
            api_nodes: Api::all(client),
            node_name,
            pending: BTreeSet::new(),
            previous: BTreeSet::new(),
            dry_run: false,
            remove_unstaged: false,
        };

        // Doubles as the "node exists and Nodes API is readable" check.
        let _ = manager.iter_node_addresses().await?;

        Ok(manager)
    }

    /// Changes whether the node addresses changing patch application
    /// should be mocked or not.
    pub fn set_dry_run(&mut self, dry_run: bool) -> &mut Self {
        self.dry_run = dry_run;
        self
    }

    /// Changes whether the current addresses attached to the node should
    /// be removed if they were not provided by [`Self::stage_address`].
    pub fn set_remove_unstaged(&mut self, remove_unstaged: bool) -> &mut Self {
        self.remove_unstaged = remove_unstaged;
        self
    }

    /// Queues (stages) the given address to add to the node as `ExternalIP`.
    /// You have to call [`Self::apply`] to apply the changes.
    pub fn stage_address(&mut self, addr: &IpAddr) -> &mut Self {
        self.pending.insert(*addr);
        self
    }

    /// Applies all the staged changes, mutating the real Node addresses.
    /// Returns the report of what changes was made.
    ///
    /// An error is returned if no addresses are staged at all. Every provider
    /// being unreachable says nothing about where the node lives, and stripping
    /// its `ExternalIP`s over a bad minute on the Internet is not an improvement.
    pub async fn apply(&mut self) -> Result<BTreeMap<IpAddr, AddrStatus>> {
        if self.pending.is_empty() {
            bail!("no addresses are staged, the node is left as it is")
        }

        let staged = mem::take(&mut self.pending);
        let mut current: Vec<NodeAddress> = self.iter_node_addresses().await?.collect();

        // A dry run never writes, so the node keeps showing the same gap and
        // every tick would cry "new" about the same address forever. Let the
        // rounds that came before pretend they landed.
        if self.dry_run {
            let attached: BTreeSet<IpAddr> = current.iter().filter_map(parse_external_ip).collect();
            current.extend(self.previous.difference(&attached).map(new_external_ip));
        }

        let mut report = BTreeMap::new();
        let mut addresses = Vec::with_capacity(current.len() + staged.len());
        let mut has_changes = false;

        // The node keeps its Hostname and InternalIP untouched; only the
        // ExternalIPs are ours to decide upon. An unparsable one is somebody
        // else's business, so it is preserved and complained about.

        for address in current {
            let Some(external_ip) = parse_external_ip(&address) else {
                addresses.push(address);
                continue;
            };

            let status = if self.remove_unstaged && !staged.contains(&external_ip) {
                has_changes = true;
                AddrStatus::Removed
            } else {
                addresses.push(address);
                AddrStatus::Skipped
            };

            report.insert(external_ip, status);
        }

        for external_ip in &staged {
            if report.contains_key(external_ip) {
                continue;
            }

            addresses.push(new_external_ip(external_ip));
            report.insert(*external_ip, AddrStatus::New);
            has_changes = true;
        }

        if has_changes {
            self.send_patch(addresses)
                .await
                .context("cannot send the patch")?;
        }

        // An address already attached before this process started is reported
        // as new once, so that a fresh operator is never silent about what
        // the node it took over is carrying.

        for (external_ip, status) in &mut report {
            if status.is_skipped() && !self.previous.contains(external_ip) {
                *status = AddrStatus::New;
            }
        }

        self.previous = report
            .iter()
            .filter(|(_, status)| !status.is_removed())
            .map(|(external_ip, _)| *external_ip)
            .collect();

        Ok(report)
    }

    /// Creates and returns an iterator that yields current `ExternalIP`
    /// addresses attached to the Node the [Manager] controls.
    pub async fn query_current_addresses(&self) -> Result<impl Iterator<Item = IpAddr> + 'static> {
        let it = self
            .iter_node_addresses()
            .await?
            .filter_map(|address| parse_external_ip(&address));

        Ok(it)
    }

    /// Prepares and applies the JSON+Merge patch that contains given addresses.
    /// It means that provided addresses replaces the current ones.
    async fn send_patch(&self, new_addresses: Vec<NodeAddress>) -> Result<Node> {
        let patch_params = PatchParams {
            dry_run: self.dry_run,
            ..PatchParams::default()
        };

        if patch_params.dry_run {
            warn!("DRY RUN REQUESTED, THE REAL NODE ADDRESSES WILL NOT BE MODIFIED");
        }

        let node = self
            .api_nodes
            .patch_status(
                &self.node_name,
                &patch_params,
                &Patch::Merge(json!({ "status": { "addresses": new_addresses } })),
            )
            .await?;

        Ok(node)
    }

    /// Creates and returns iterator over all the addresses of the node,
    /// the [Manager] controls.
    /// The output contain all the addresses, including `InternalIP` and `Hostname`.
    async fn iter_node_addresses(&self) -> Result<impl Iterator<Item = NodeAddress> + 'static> {
        // About 'static in return:
        // https://blog.rust-lang.org/2024/09/05/impl-trait-capture-rules/

        let addrs = self
            .api_nodes
            .get(&self.node_name)
            .await
            .context("cannot query the requested Node")?
            .status
            .and_then(|status| status.addresses)
            .unwrap_or_default()
            .into_iter();

        Ok(addrs)
    }
}

/// Returns the parsed address if it is an `ExternalIP`, [`None`] otherwise.
/// An `ExternalIP` that fails to parse is reported and treated as not ours.
fn parse_external_ip(node_address: &NodeAddress) -> Option<IpAddr> {
    if node_address.type_ != Manager::TYPE_EXTERNAL_IP {
        return None;
    }

    IpAddr::from_str(&node_address.address)
        .inspect_err(|err| warn!(address = node_address.address, %err, "unparsable ExternalIP"))
        .ok()
}

fn new_external_ip(ip: &IpAddr) -> NodeAddress {
    NodeAddress {
        address: ip.to_string(),
        type_: Manager::TYPE_EXTERNAL_IP.into(),
    }
}
