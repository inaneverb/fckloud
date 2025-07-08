use {
    anyhow::{Context, Result},
    k8s_openapi::api::core::v1::{Node, NodeAddress},
    kube::{
        Api, Client, Config,
        api::{Patch, PatchParams},
    },
    serde_json::json,
    std::{
        collections::BTreeSet,
        fmt::{Debug, Display},
        net::IpAddr,
        str::FromStr,
        time::Duration,
    },
    tracing::{debug, info, instrument, warn},
};

pub struct Manager {
    api_nodes: Api<Node>,
    node_name: String,
}

// [Handle] is the endpoint for managing Node addresses,
// allowing adding new ones or removing stale entries.
//
// It holds a cached copy of addresses at creation time,
// so operations should be done quickly to avoid external changes being made.
//
// Call [refresh] to retrieve up-to-date data.
pub struct Handle<'a> {
    manager: &'a Manager,
    dry_run: bool,

    external_ips_were: BTreeSet<IpAddr>,
    external_ips_added: BTreeSet<IpAddr>,
    non_external_ips: Vec<NodeAddress>, // RAW addr -> Type
}

impl Manager {
    const TYPE_INTERNAL_IP: &'static str = "InternalIP";
    const TYPE_EXTERNAL_IP: &'static str = "ExternalIP";

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
            .inspect(|(version, host)| {
                info!(version, host, "connected to the Kubernetes cluster")
            })?;

        // We can create Manager at this point.
        // Do it but also check that Nodes API is accessible.

        let manager = Self::new_with_api(Api::all(client.clone()), node_name);

        manager
            .iter_node_addresses()
            .await?
            .filter(is_external_ip)
            .filter_map(|node_address| parse_ip(&node_address))
            .for_each(|node_ip| {
                debug!(
                    ?node_ip,
                    "this ExternalIP is currently attached to the node"
                );
            });

        Ok(manager)
    }

    /// This is simplified private constructor with no internal fallable checks.
    /// Just returns [Manager] based on the given arguments.
    fn new_with_api(api_nodes: Api<Node>, node_name: String) -> Self {
        Self {
            api_nodes,
            node_name,
        }
    }

    pub async fn get_handle(&self, dry_run: bool) -> Result<Handle> {
        Handle::new(self, dry_run).await
    }

    fn iter_node_addresses_for(node: Node) -> impl Iterator<Item = NodeAddress> {
        node.status
            .and_then(|status| status.addresses)
            .unwrap_or_default()
            .into_iter()
    }

    /// Creates and returns an iteratorover all InternalIP and ExternalIP
    /// addresses of the node, typed as [NodeAddress].
    async fn iter_node_addresses(&self) -> Result<impl Iterator<Item = NodeAddress>> {
        // Ok(self
        //     .api_nodes
        //     .get(&self.node_name)
        //     .await
        //     .with_context(|| format!("cannot query the requested Node"))?
        //     .status
        //     .and_then(|status| status.addresses)
        //     .unwrap_or_default()
        //     .into_iter())

        Ok(Self::iter_node_addresses_for(
            self.api_nodes
                .get(&self.node_name)
                .await
                .with_context(|| format!("cannot query the requested Node"))?,
        ))

        // .filter(|e1| matches!(e1.type_.as_str(), "InternalIP" | "ExternalIP")))
    }

    // async fn iter_ip_addresses(&self) -> Result<impl Iterator<Item = IpAddr>> {
    //     Ok(self.iter_node_addresses().await?.filter_map(only_ip))
    // }

    // async fn iter_external_ip_addresses(&self) -> Result<impl Iterator<Item=IpAddr>> {
    //     Ok(self.iter_node_addresses().await?.filter_map(only_external_ip))
    // }

    // Helper to get the Kubernetes config, with some defaults possibly overridden.
    async fn get_config() -> Result<Config> {
        const CONNECTION_TIMEOUT: Duration = Duration::from_secs(5);

        let mut config = Config::infer().await?;
        config.connect_timeout = CONNECTION_TIMEOUT.into();
        Ok(config)
    }
}

impl<'a> Handle<'a> {
    async fn new(manager: &'a Manager, dry_run: bool) -> Result<Handle<'a>> {
        let mut handle = Self {
            manager,
            dry_run,
            external_ips_added: BTreeSet::new(),
            external_ips_were: BTreeSet::new(),
            non_external_ips: Vec::new(),
        };
        handle.refresh().await?;
        Ok(handle)
    }

    /// Removes ExternalIP addresses from the node that weren't set
    /// via [apply_addr] during the [Handle]'s lifetime.
    ///
    /// WARNING:
    /// Calling this before any [apply_addr] calls will remove all
    /// the ExternalIP addresses from the node!
    #[instrument(skip(self))]
    pub async fn remove_unapplied(&mut self) -> Result<Vec<IpAddr>> {
        // let mut new: Vec<NodeAddress> = Vec::new();
        // let mut discarded: Vec<IpAddr> = Vec::new();

        // for node_address in self.manager.iter_node_addresses().await? {
        //     // In case you've found it hard to understand,
        //     // Discarded: is ExternalIP && addr is parsed && not in "applied"

        //     if let Some(parsed_ip) = parse_ip(&node_address) {
        //         if is_external_ip(&node_address) && !self.applied.contains(&parsed_ip) {
        //             discarded.push(parsed_ip);
        //         } else {
        //             new.push(node_address);
        //         }
        //     } else {
        //         new.push(node_address);
        //     }
        // }

        // Prepare the list of addresses of all non ExternalIPs
        // + these that were added explicitly.

        let mut new = self.non_external_ips.clone();

        for ip_added in self.external_ips_added.iter() {
            new.push(new_node_address(ip_added, Manager::TYPE_EXTERNAL_IP));
        }

        self.apply_patch(new).await?;

        self.external_ips_were
            .iter()
            .for_each(|node_ip| warn!(?node_ip, "ExternalIP was discarded from the Node"));

        Ok(self.external_ips_were.clone().into_iter().collect())
    }

    pub async fn apply_addrs(&mut self, addr: &IpAddr) -> Result<()> {
        if self.external_ips_added.contains(addr) || self.external_ips_were.contains(addr) {
            return Ok(());
        }

        let mut new = self.non_external_ips.clone();
        new.push(new_node_address(addr, Manager::TYPE_EXTERNAL_IP));

        for ip_was in self.external_ips_were.iter() {
            new.push(new_node_address(ip_was, Manager::TYPE_EXTERNAL_IP));
        }

        self.apply_patch(new).await?;
        self.external_ips_added.insert(*addr);

        Ok(())
    }

    // Retrieves and caches Node addresses.
    // This call is always made implicitly when [Handle] is created.
    //
    // You may want to call it manually,
    // to make sure having the actual cached state of addresses at some point.
    //
    // The track of added addresses persist betwixt calls of [refresh],
    // although if just added address was removed recently
    // it will be also removed from the cache as well.
    pub async fn refresh(&mut self) -> Result<()> {
        // The only one thing that complicates the process a bit,
        // is [refresh] call in-between the other calls, eg
        // - Between two [apply_addrs]
        // - Between [apply_addrs] or [remove_unapplied].
        //
        // That's why we want:
        // 1. Discard all previous non ExternalIP addresses from cache,
        //    overwriting it by the just retrieved set of them.
        //
        // 2. Remove the ExternalIP that was applied previously by us,
        //    but not found in the new set.
        //    In general, it means:
        //    - Either the call of adding such address was with "dry run",
        //    - or the address was removed recently,
        //      thus we definitely do not want to "re-apply" it.
        //
        // 3. Treat the remaining ExternalIP as well as non ExternalIP addresses
        //    ie overwrite cached by the just retrieved.
        //

        // For now save all ExternalIPs into "were" addresses.
        // We will remove these that are applied by us.

        self.external_ips_were.clear();
        self.non_external_ips.clear();

        for node_address in self.manager.iter_node_addresses().await? {
            if is_external_ip(&node_address)
                && let Some(parsed_ip) = parse_ip(&node_address)
            {
                self.external_ips_were.insert(parsed_ip);
            } else {
                self.non_external_ips.push(node_address);
            }
        }

        // Remove these applied addresses, that has not been persisted
        // thus were removed recently and we don't need them cached.
        // Then remove the added ones from the "were" list.

        self.external_ips_added
            .retain(|ip_added| self.external_ips_were.contains(ip_added));

        self.external_ips_were
            .retain(|ip_was| !self.external_ips_added.contains(ip_was));

        Ok(())
    }

    // apply_patch just applies JSON+Merge patch that contains given addresses.
    // It means that provided addresses replaces the current ones.
    async fn apply_patch(&self, new_addresses: Vec<NodeAddress>) -> Result<Node> {
        let mut patch_params = PatchParams::default();
        patch_params.dry_run = self.dry_run;

        if patch_params.dry_run {
            warn!("DRY RUN REQUESTED, THE NODE ADDRESSES WILL REMAIN THE SAME");
        }

        let node = self
            .manager
            .api_nodes
            .patch_status(
                &self.manager.node_name,
                &patch_params,
                &Patch::Merge(json!({ "addresses": new_addresses })),
            )
            .await?;

        Ok(node)
    }
}

// trait Example where Self: Sized + Iterator<Item = NodeAddress> {
//     fn to_ip_addrs(self) -> impl Iterator<Item = IpAddr> {
//         self.filter_map(only_ip)
//     }
//     fn only_external_ips(self) -> impl Iterator<Item=NodeAddress> {
//         self.filter(is_external_ip)
//     }
//     fn only_internal_ips(self) -> impl Iterator<Item = NodeAddress> {
//         self.filter(|node| matches!(node.address.as_str(), "InternalIP"))
//     }
//     fn only_ips(self) -> impl Iterator<Item = NodeAddress> {
//         self.only_external_ips().only_internal_ips()
//     }
// }

// impl<T> Example for T where T: Iterator<Item = NodeAddress> {}

// impl<T> Example for T where T:  {
//     fn to_ip_addrs(self) -> impl Iterator<Item = IpAddr> {
//     }
//     fn only_external_ips(self) -> impl Iterator<Item=NodeAddress> {
//     }
// }

// #[instrument]
// fn only_external_ip(node_address: NodeAddress) -> Option<IpAddr> {
//     match node_address.type_.as_str() {
//         "ExternalIP" => only_ip(node_address),
//         _ => None
//     }
// }

#[inline(always)]
fn is_internal_ip(node_address: &NodeAddress) -> bool {
    node_address.type_ == Manager::TYPE_INTERNAL_IP
}

#[inline(always)]
fn is_external_ip(node_address: &NodeAddress) -> bool {
    node_address.type_ == Manager::TYPE_EXTERNAL_IP
}

#[inline(always)]
fn is_internal_external_ip(node_address: &NodeAddress) -> bool {
    is_internal_ip(node_address) || is_external_ip(node_address)
}

#[instrument]
fn parse_ip(node_address: &NodeAddress) -> Option<IpAddr> {
    is_internal_external_ip(&node_address)
        .then(|| {
            IpAddr::from_str(&node_address.address)
                .inspect_err(|err| warn!("unable to parse Node address {}", err))
                .ok()
        })
        .flatten()
}

#[inline(always)]
fn new_node_address(ip: &IpAddr, type_: &str) -> NodeAddress {
    NodeAddress {
        address: ip.to_string(),
        type_: type_.into(),
    }
}
