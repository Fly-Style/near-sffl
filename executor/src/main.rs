mod types;
mod executor;
mod config;

use crate::executor::{Executor, Packet, PacketSent};
use alloy::primitives::Address;
use alloy::{
    providers::Provider,
    providers::ProviderBuilder,
    providers::WsConnect,
    rpc::types::{BlockNumberOrTag, Filter},
};
use alloy_sol_types::*;
use eyre::Result;
use futures::stream::StreamExt;
use serde::{Deserialize, Serialize};

#[tokio::main]
async fn main() -> Result<()> {
    // Create the provider.
    let rpc_url = "wss://arbitrum-mainnet.infura.io/ws/v3/a7d4a3dd6f774049bce5d61651549421";
    let ws = WsConnect::new(rpc_url);
    let provider = ProviderBuilder::new().on_ws(ws).await?;

    // Create a filter for the packet-sent events.
    let addresses = vec![
        // LayerZero endpoint
        "0x1a44076050125825900e736c501f859c50fE728c".parse::<Address>()?,
        "0x975bcD720be66659e3EB3C0e4F1866a3020E493A".parse::<Address>()?,
    ];

    let packet_sent_filter = Filter::new()
        .address(addresses.clone())
        .event("PacketSent(bytes,bytes,address)") // endpoint
        // .event("ExecutorFeePaid(address,uint256);") // endpoint
        // .event("PayloadVerified(address,bytes,uint256,bytes32);") // endpoint
        .from_block(BlockNumberOrTag::Latest);


    let packet_sent_executor = Executor::new(rpc_url.clone(), packet_sent_filter);

    Ok(())
}