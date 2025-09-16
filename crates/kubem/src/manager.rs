use {
    anyhow::{Context, Result, bail},
    ekacore::traits::Discard,
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
        ops::Not,
        str::FromStr,
        time::Duration,
    },
    strum_macros::{EnumIs, EnumString},
    tracing::{debug, instrument, warn},
};

pub struct Manager {
    api_nodes: Api<Node>,
    node_name: String,

    dry_run: bool,
    remove_unapplied: bool,

    pending: BTreeSet<IpAddr>,
    previous: BTreeSet<IpAddr>,
}

#[derive(EnumIs, EnumString)]
pub enum AddrStatus {
    New,
    Skipped,
    Removed,
}

impl Manager {
    const TYPE_INTERNAL_IP: &'static str = "InternalIP";
    const TYPE_EXTERNAL_IP: &'static str = "ExternalIP";

    // Creates and returns a [Manager] that connects to the Kubernetes cluster.
    // Verifies the connection, ensures the given `node_name` exists,
    // and that the Nodes API is accessible.
    // Returns an error if any check fails. Panics if `node_name` is empty.
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
                debug!(version, host, "connected to the Kubernetes cluster")
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
            pending: BTreeSet::new(),
            previous: BTreeSet::new(),
            dry_run: false,
            remove_unapplied: false,
        }
    }

    // Changes whether the node addresses changing patch application
    // should be mocked or not.
    pub fn set_dry_run(&mut self, dry_run: bool) -> &mut Self {
        self.dry_run = dry_run;
        self
    }

    // Changes whether the current addresses attached to the node should
    // be removed if they were not provided by [stage_address].
    pub fn set_remove_unstaged(&mut self, remove_unstaged: bool) -> &mut Self {
        self.remove_unapplied = remove_unstaged;
        self
    }

    // Queues (stages) the given address to add to the node as ExternalIP.
    // You have to call [apply] to apply the changes.
    pub fn stage_address(&mut self, addr: &IpAddr) -> &mut Self {
        self.pending.insert(*addr);
        self
    }

    // Applies all the staged changes, mutating the real Node addresses.
    // Returns the report of what changes was made.
    //
    // An error is returned if no addresses are staged and removing unapplied
    // is not requested, thus preventing you from removing all ExternalIP
    // addresses of node accidentally.
    pub async fn apply(&mut self) -> Result<BTreeMap<IpAddr, AddrStatus>> {
        if self.pending.is_empty() && !self.remove_unapplied {
            bail!("no addresses are staged and remove unapplied is not requested")
        }

        let mut out = BTreeMap::new();
        let mut patch = Vec::new();
        let mut has_changes = false;

        // A lot is going on here with some tricks, so brief explanation.
        //
        // We will iterate over CURRENT node addresses,
        // wanting to get such an array at the end
        // that will have these CURRENT node addresses that must be preserved.
        //
        // At the same time we will be adding both non-preserved (removed)
        // and preserved (skipped) addresses to the report.
        //
        // 1. If it's NOT ExternalIP (either Hostname or InternalIP)
        //    it must be preserved, will be a part of the array
        //    but will NOT be added to the report at all;
        //
        // 2. If it's an ExternalIP, try and remove it from the pending addresses
        //    (these user have marked explicitly as ones they want the node to have)
        //
        // 2.1. If succeeded (address was in pending),
        //      this address must be preserved, will be a part of the array
        //      and will be added to the report as skipped one;
        //
        // 2.2. If failed (address was not in pending),
        //
        // 2.2.1. If user requested strict mode
        //        (remove unconfirmed (not marked explicitly) addresses from the node)
        //        it must be FILTERED OUT, will NOT be a part of the array,
        //        but will be added to the report as removed one;
        //
        // 2.2.2. Not strict mode so,
        //        will be a part of the array,
        //        will be added to the report as skipped one;
        //
        // So, the resulting array of NodeAddress will have:
        // - Both Hostname and InternalIP
        // - These ExternalIP that has "skipped" status in the report.
        //
        // Once again, we are iterating over CURRENT node addresses,
        // not over these that user requested to add to the node.
        //
        // We also using pseudo CURRENT node addresses instead of real ones
        // if it's dry run mode (read more about it far below).

        let part_1: Vec<NodeAddress> = if self.dry_run.not() {
            self.iter_node_addresses().await?.collect()
        } else {
            self.previous
                .iter()
                .map(|addr| new_node_address(&addr, Self::TYPE_EXTERNAL_IP))
                .collect()
        };

        let part_1 = part_1
            .into_iter()
            .filter(|node_address| match parse_ip(&node_address) {
                _ if is_external_ip(&node_address).not() => true,
                None => unreachable!("is an external IP that must be parsed"),
                Some(external_ip) => {
                    let status = match self.pending.remove(&external_ip) {
                        false if self.remove_unapplied => {
                            has_changes = true;
                            AddrStatus::Removed
                        }
                        _ => AddrStatus::Skipped,
                    };
                    out.entry(external_ip).or_insert(status).is_skipped()
                }
            });

        patch.extend(part_1);

        // Because we were trying and removing currently attached ExternalIPs
        // from the pending addresses, now it has only brand-new these.
        // So add them all as new to the report and to the patch as well.

        let mut part_2 = mem::take(&mut self.pending)
            .into_iter()
            .inspect(|ip| out.insert(*ip, AddrStatus::New).discard())
            .map(|ip| new_node_address(&ip, Self::TYPE_EXTERNAL_IP))
            .peekable();

        has_changes = has_changes || part_2.peek().is_some();
        patch.extend(part_2);

        if has_changes {
            self.send_patch(patch)
                .await
                .with_context(|| format!("cannot send the patch"))?;
        }

        // For strictly cosmetic purposes, we want to consider addresses
        // that are currently attached and were preserved
        // as new ones at least once.
        //
        // That way the caller might have them logged,
        // preventing confusing silent mode when all addresses are
        // confirmed and skipped.
        //
        // That's why we are maintaining the set of addresses of "previous",
        // that is empty at the beginning.

        out = out
            .into_iter()
            .map(|(address, status)| match status {
                AddrStatus::Skipped if !self.previous.contains(&address) => {
                    (address, AddrStatus::New)
                }
                _ => (address, status),
            })
            .collect();

        out.iter().for_each(|(address, status)| match status {
            AddrStatus::Removed => self.previous.remove(address).discard(),
            _ => self.previous.insert(*address).discard(),
        });

        Ok(out)
    }

    // Creates and returns an iterator that yeilds current ExternalIP
    // addresses attached to the Node the [Manager] controls.
    pub async fn query_current_addresses(&self) -> Result<impl Iterator<Item = IpAddr> + 'static> {
        let it = self
            .iter_node_addresses()
            .await?
            .filter(is_external_ip)
            .filter_map(|node_address| parse_ip(&node_address));

        Ok(it)
    }

    // Prepares and applies the JSON+Merge patch that contains given addresses.
    // It means that provided addresses replaces the current ones.
    async fn send_patch(&self, new_addresses: Vec<NodeAddress>) -> Result<Node> {
        let mut patch_params = PatchParams::default();
        patch_params.dry_run = self.dry_run;

        if patch_params.dry_run {
            warn!("DRY RUN REQUESTED, THE REAL NODE ADDRESSES WILL NOT BE MODIFIED");
        }

        let node = self
            .api_nodes
            .patch_status(
                &self.node_name,
                &patch_params,
                &Patch::Merge(json!({ "addresses": new_addresses })),
            )
            .await?;

        Ok(node)
    }

    // Creates and returns iterator over all the addresses of the node,
    // the [Manager] controls.
    // The output contain all the addresses, including InternalIP and Hostname.
    async fn iter_node_addresses(&self) -> Result<impl Iterator<Item = NodeAddress> + 'static> {
        // About 'static in return:
        // https://blog.rust-lang.org/2024/09/05/impl-trait-capture-rules/

        let addrs = self
            .api_nodes
            .get(&self.node_name)
            .await
            .with_context(|| format!("cannot query the requested Node"))?
            .status
            .and_then(|status| status.addresses)
            .unwrap_or_default()
            .into_iter();

        Ok(addrs)
    }

    // Helper to get the Kubernetes config, with some defaults overridden.
    async fn get_config() -> Result<Config> {
        const CONNECTION_TIMEOUT: Duration = Duration::from_secs(5);

        let mut config = Config::infer().await?;
        config.connect_timeout = CONNECTION_TIMEOUT.into();
        Ok(config)
    }
}

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
