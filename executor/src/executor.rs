//! 1. Both Committer and Executor roles first listen for the `PacketSent` event:
//! `PacketSent(bytes encodedPacket, bytes options, address sendLibrary);`
//!
//! 2. After the PacketSent event, the ExecutorFeePaid is how you know your
//! Executor/Committer has been assigned to commit and execute the packet.
//! `ExecutorFeePaid(address executor, uint256 fee);`
//!
//! 3. After receiving the fee, your `Executor`/`Committer` should listen for
//! the `PacketVerified` event, signaling that the packet can now be committed
//! to the destination messaging channel:
//! `PayloadVerified(address dvn, bytes header, uint256 confirmations,  bytes32 proofHash)`;
//!
//! 4. After listening for the previous events, your `Executor`/`Committer` should perform
//! an idempotency check, but in a different ways:
//!
//! 4.1. `Committer` calls `Ultra Light Node 301` and `Ultra Light Node 302`:
//! `ULN.verifiable(_packetHeader, _payloadHash);`
//! This function will return the following possible states:
//! `enum VerificationState {
//!   Verifying,
//!   Verifiable,
//!   Verified
//! }`
//!
//! If the state is `Verifying`, your `Executor` must wait for more DVNs to sign
//! the packet's `payloadHash`. After a DVN signs the `payloadHash`, it will emit `PayloadVerified`:
//! `PayloadVerified(address dvn, bytes header, uint256 confirmations, bytes32 proofHash);`
//!
//! If the state is `Verifiable`, then your `Executor` must call `commitVerification`:
//! `function commitVerification(bytes calldata _packetHeader, bytes32 _payloadHash) external;`
//!
//! If the state is `Verified`, the commit has already occurred and the commit workflow
//! can be terminated.
//!
//! 4.2. `Executor` should perform an idempotency check:
//! `endpoint.executable(_packetHeader, _payloadHash);`
//! This function will return the following possible states:
//! `enum ExecutionState {
//!   NotExecutable,
//!   Executable,
//!   Executed
//! }`
//!
//! If the state is `NotExecutable`, your Executor must wait for the `Committer` to commit
//! the message packet, or you may have to wait for some previous nonces.
//!
//! If the state is `Executable`, your Executor should decode the packet's options using the
//! `options.ts` package and call the Endpoint's `lzReceive` function with the packet information:
//! `endpoint.lzReceive( _origin, _receiver, _guid, _message, _extraData)`
//!
//! If the state is `Executed`, your Executor has fulfilled its obligation,
//! and you can terminate the `Executor` workflow.

use crate::types::SolidityAddress;
use alloy::primitives::Address;
use alloy::providers::{Provider, ProviderBuilder, WsConnect};
use alloy::rpc::types::Filter;
use serde::{Deserialize, Serialize};
use eyre::Result;
use futures::StreamExt;

#[derive(Debug)]
pub struct Packet {
    nonce: u64,
    src_eid: u32,
    sender: Address,
    dst_eid: u32,
    receiver: [u8; 32],
    guid: [u8; 32],
    message: Vec<u8>,
}

impl Packet {
    pub fn deserialize(data: &[u8]) -> Option<Self> {
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

    pub fn message(&self) -> &[u8] {
        &self.message.as_slice()
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

    pub fn deserialize(data: &[u8]) -> Option<Self> {
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

        Some(PacketSent::new(encoded_packet, options, send_library))
    }
}

pub(crate) struct ListenerContext {}

pub struct Executor {
    pub(crate) url: String,
    pub(crate) filter: Filter,
}

impl Executor {
    pub fn new(url: &str, filter: Filter, ) -> Self {
        Executor {  url: String::from(url), filter }
    }

    pub async fn listen<T>(&self, deserializer: fn(&[u8]) -> T) -> Result<()> {
        let ws = WsConnect::new(&self.url);
        let provider = ProviderBuilder::new().on_ws(ws).await?;

        let sub = provider.subscribe_logs(&self.filter).await?;
        let mut stream = sub.into_stream();

        while let Some(log) = stream.next().await {
            match Packet::deserialize(log.data().data.iter().as_slice()) {
                Some(packet) => {
                    match deserializer(packet.message()) {
                        Some(packet_sent) => {
                            println!("PacketSent: {:?}", packet_sent);
                        }
                        None => {
                            println!("Not a PacketSent :(");
                        }
                    }
                }
                None => {
                    // TODO: fire a warn/info to a normal logger.
                    println!("Expected Packet, but got other blob. Fail.");
                }
            }
        }

        Ok(())
    }
}