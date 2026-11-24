//! The cluster's nodes over `x:ClusterNode`, read only (see `dev/notes/stalwart-admin-more.md`):
//! a node's number, host name, status (`active`, `stale` or `inactive`) and the time of its last
//! lease renewal. A server that is not in a cluster has the one node it is.

use serde_json::json;

use super::{count, text, time};
use crate::{
    ffi::ClusterNode,
    jmap::{send::response, Connection},
    Result,
};

impl Connection {
    /// The nodes by number.
    pub(crate) async fn cluster_nodes(&self) -> Result<Vec<ClusterNode>> {
        let args = json!({ "accountId": self.account_id(), "ids": null });
        let responses = self.admin_request("see the cluster", &[("x:ClusterNode/get", args)]).await?;
        let mut nodes: Vec<ClusterNode> = response(&responses, "x:ClusterNode/get")?["list"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|n| {
                Some(ClusterNode {
                    id: n["id"].as_str()?.to_string(),
                    node_id: count(&n["nodeId"]),
                    hostname: text(n, "hostname"),
                    status: text(n, "status"),
                    last_renewal: time(&n["lastRenewal"]),
                })
            })
            .collect();
        nodes.sort_by(|a, b| a.node_id.cmp(&b.node_id).then_with(|| a.id.cmp(&b.id)));
        Ok(nodes)
    }
}
