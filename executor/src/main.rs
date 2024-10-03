mod types;
mod executor;

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
use types::SolidityAddress;

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
        .event("PacketSent(bytes,bytes,address)") // this is from the endpoint addr
        .from_block(BlockNumberOrTag::Latest);

    // Subscribe to logs.
    let sub = provider.subscribe_logs(&filter_l0).await?;

    let mut stream = sub.into_stream();

    println!("Logs:");
    while let Some(log) = stream.next().await {
        let log_data: &[u8] = log.data().data.iter().as_slice();
        let packet_opt = Packet::deserialize(log_data);
        match packet_opt {
            Some(packet) => {
                println!("Packet : {:?}", packet);
                let packet_sent_opt = PacketSent::deserialize(packet.message.as_slice());
                match packet_sent_opt {
                    Some(packet_sent) => {
                        println!("PacketSent : {:?}", packet_sent);
                    },
                    None => {
                        println!("Failed to deserialize PacketSent");
                    }
                }
            },
            None => {
                println!("Failed to deserialize packet");
            }
        }
    }

    Ok(())
}

#[derive(Debug)]
struct Packet {
    nonce: u64,
    src_eid: u32,
    sender: Address,
    dst_eid: u32,
    receiver: [u8; 32],
    guid: [u8; 32],
    message: Vec<u8>,
}

impl Packet {
    fn deserialize(data: &[u8]) -> Option<Self> {
        if data.len() < 100 { // Minimum length: 8 + 4 + 20 + 4 + 32 + 32 = 100, plus some for message
            return None;
        }

        let nonce = u64::from_be_bytes(data[0..8].try_into().ok()?);
        let src_eid = u32::from_be_bytes(data[8..12].try_into().ok()?);
        let sender: [u8; 20] = data[12..32].try_into().ok()?;
        let sender = Address::from_slice(&sender);
        let dst_eid = u32::from_be_bytes(data[32..36].try_into().ok()?);
        let receiver: [u8; 32] = data[36..68].try_into().ok()?;
        let guid: [u8; 32] = data[68..100].try_into().ok()?;
        let message = data[100..].to_vec();

        Some(Packet {
            nonce,
            src_eid,
            sender,
            dst_eid,
            receiver,
            guid,
            message,
        })
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PacketSent {
    #[serde(rename = "encodedPacket")]
    pub(crate) encoded_packet: Vec<u8>,
    pub(crate) options: Vec<u8>,
    #[serde(rename = "sendLibrary")]
    pub(crate) send_library: SolidityAddress,
}

impl PacketSent {
    fn new(encoded_packet: Vec<u8>, options: Vec<u8>, send_library: [u8; 20]) -> Self {
        PacketSent { encoded_packet, options, send_library }
    }

    fn deserialize(data: &[u8]) -> Option<Self> {
        if data.len() < 52 {
            return None;
        }

        // Extract the send_library address (last 20 bytes)
        let send_library: SolidityAddress = data[data.len() - 20..]
            .try_into()
            .expect("Failed to extract send_library address");

        let remaining_data = &data[..data.len() - 20];
        let encoded_packet_length = 32; // Assuming a fixed length for encoded_packet

        if remaining_data.len() < encoded_packet_length {
            return None;
        }

        let encoded_packet = remaining_data[..encoded_packet_length].to_vec();
        let options = remaining_data[encoded_packet_length..].to_vec();

        Some(PacketSent {
            encoded_packet,
            options,
            send_library,
        })
    }
}