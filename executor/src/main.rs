mod types;
mod executor;

use crate::executor::{Packet, PacketSent};
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

    let filter_l0 = Filter::new()
        .address(addresses.clone())
        .event("PacketSent(bytes,bytes,address)") // endpoint
        // .event("PayloadVerified(address,bytes,uint256,bytes32);") // endpoint
        .from_block(BlockNumberOrTag::Latest);

    // Subscribe to logs.
    let sub = provider.subscribe_logs(&filter_l0).await?;
    let mut stream = sub.into_stream();

    while let Some(log) = stream.next().await {
        match Packet::deserialize(log.data().data.iter().as_slice()) {
            Some(packet) => {
                println!("Packet: {:?}", log);
                // I saw a great example for handling futures with tokio::select! macro
                tokio::select! {
                    Some(packet_sent) = async { PacketSent::deserialize(packet.message()) } => {
                        println!("PacketSent: {:?}", log);
                    }
                    else => {
                        break;
                    }
                };
            }
            None => {
                // TODO: fire a warn/info to a normal logger.
                println!("Expected Packet, but got other blob. Fail.");
            }
        }
    }

    Ok(())
}