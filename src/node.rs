mod reconcile;

pub use self::reconcile::AddrStatus;

use {
    self::reconcile::{new_external_ip, parse_external_ip, reconcile},
    anyhow::{Context, Result, bail},
    k8s_openapi::api::core::v1::{Node, NodeAddress},
    kube::{
        Api, Client, Config,
        api::{Patch, PatchParams},
    },
    serde_json::json,
    std::{
        collections::{BTreeMap, BTreeSet},
        net::IpAddr,
        time::Duration,
    },
    tracing::{Span, debug, field::Empty, instrument, warn},
};

/// Owns the Kubernetes side: reads the Node, hands the decision to
/// [`reconcile`], and writes back whatever it decided.
pub struct Manager {
    api_nodes: Api<Node>,
    node_name: String,

    dry_run: bool,
    remove_unstaged: bool,

    previous: BTreeSet<IpAddr>,
}

impl Manager {
    const CONNECTION_TIMEOUT: Duration = Duration::from_secs(5);

    /// Connects to the cluster, verifies the connection and that the given node
    /// exists and its status is ours to write. Panics if `node_name` is empty.
    pub async fn new(node_name: &str) -> Result<Self> {
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
            node_name: node_name.to_owned(),
            dry_run: false,
            remove_unstaged: false,
            previous: BTreeSet::new(),
        };

        // Doubles as the "node exists and Nodes API is readable" check.
        let _ = manager.node_addresses().await?;

        Ok(manager)
    }

    pub fn set_dry_run(&mut self, dry_run: bool) -> &mut Self {
        self.dry_run = dry_run;
        self
    }

    pub fn set_remove_unstaged(&mut self, remove_unstaged: bool) -> &mut Self {
        self.remove_unstaged = remove_unstaged;
        self
    }

    /// Brings the node's `ExternalIP`s in line with the given addresses and
    /// reports what changed.
    ///
    /// An error is returned if nothing is staged. Every provider being
    /// unreachable says nothing about where the node lives, and stripping its
    /// `ExternalIP`s over a bad minute on the Internet is not an improvement.
    #[instrument(name = "node.apply", skip_all, fields(
        fckloud.node.added = Empty,
        fckloud.node.kept = Empty,
        fckloud.node.removed = Empty,
    ))]
    pub async fn apply(
        &mut self,
        staged: &BTreeSet<IpAddr>,
    ) -> Result<BTreeMap<IpAddr, AddrStatus>> {
        if staged.is_empty() {
            bail!("no addresses are staged, the node is left as it is")
        }

        let mut current = self.node_addresses().await?;

        // A dry run never writes, so the node keeps showing the same gap and
        // every tick would cry "new" about the same address forever. Let the
        // rounds that came before pretend they landed.
        if self.dry_run {
            let attached: BTreeSet<IpAddr> = current.iter().filter_map(parse_external_ip).collect();
            current.extend(self.previous.difference(&attached).map(new_external_ip));
        }

        let outcome = reconcile(current, staged, self.remove_unstaged);

        if outcome.has_changes {
            self.send_patch(outcome.addresses)
                .await
                .context("cannot send the patch")?;
        }

        let mut report = outcome.report;

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

        let tally = |wanted: fn(&AddrStatus) -> bool| report.values().filter(|s| wanted(s)).count();
        Span::current()
            .record("fckloud.node.added", tally(AddrStatus::is_new))
            .record("fckloud.node.kept", tally(AddrStatus::is_skipped))
            .record("fckloud.node.removed", tally(AddrStatus::is_removed));

        Ok(report)
    }

    /// The `ExternalIP`s currently attached to the node.
    pub async fn current_external_ips(&self) -> Result<Vec<IpAddr>> {
        let it = self
            .node_addresses()
            .await?
            .iter()
            .filter_map(parse_external_ip)
            .collect();

        Ok(it)
    }

    /// Sends the addresses as a merge patch against the `status` subresource.
    /// The `status` wrapper is not optional: without it the API server treats
    /// `addresses` as an unknown field and answers "patched (no change)".
    #[instrument(name = "k8s.node.patch_status", skip_all, fields(otel.kind = "client"))]
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

    /// Every address on the node, `InternalIP` and `Hostname` included.
    #[instrument(name = "k8s.node.get", skip_all, fields(otel.kind = "client"))]
    async fn node_addresses(&self) -> Result<Vec<NodeAddress>> {
        let addrs = self
            .api_nodes
            .get(&self.node_name)
            .await
            .context("cannot query the requested Node")?
            .status
            .and_then(|status| status.addresses)
            .unwrap_or_default();

        Ok(addrs)
    }
}
