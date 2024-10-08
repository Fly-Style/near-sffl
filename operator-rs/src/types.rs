use std::collections::HashMap;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct NodeConfig {
    /// Used to set the logger level (true = info, false = debug)
    pub production: bool,
    pub operator_address: String,
    pub operator_state_retriever_address: String,
    pub avs_registry_coordinator_address: String,
    pub token_strategy_addr: String,
    pub eth_rpc_url: String,
    pub eth_ws_url: String,
    pub bls_private_key_store_path: String,
    pub ecdsa_private_key_store_path: String,
    pub aggregator_server_ip_port_address: String,
    pub register_operator_on_startup: bool,
    pub eigen_metrics_ip_port_address: String,
    pub enable_metrics: bool,
    pub node_api_ip_port_address: String,
    pub enable_node_api: bool,
    pub near_da_indexer_rmq_ip_port_address: String,
    pub near_da_indexer_rollup_ids: Vec<u32>,
    pub rollup_ids_to_rpc_urls: HashMap<u32, String>,
    pub task_response_wait_ms: u32,
}