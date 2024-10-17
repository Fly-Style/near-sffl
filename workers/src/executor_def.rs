use crate::abi::L0V2EndpointAbi::{Origin, PacketSent, PacketVerified};
use crate::abi::SendLibraryAbi::ExecutorFeePaid;
use crate::chain::{connections, contracts, ContractInst, HttpProvider};
use crate::config::DVNConfig;
use alloy::contract::Error;
use alloy::contract::Error::{PendingTransactionError, TransportError};
use alloy::dyn_abi::{DynSolValue, DynSolValue::CustomStruct};
use alloy::network::Ethereum;
use alloy::primitives::bytes::{Buf, BytesMut};
use alloy::primitives::{FixedBytes, I256, U256};
use alloy::providers::RootProvider;
use alloy::pubsub::PubSubFrontend;
use eyre::Result;
use futures::StreamExt;
use log::info;
use std::cell::Cell;
use std::collections::VecDeque;
use tokio::time::{sleep, Duration};
use tracing::{debug, error};

pub struct Executor {
    config: DVNConfig,
    finish: Cell<bool>,
}

impl Executor {
    pub fn new(config: DVNConfig) -> Self {
        Executor {
            config,
            finish: Cell::new(false),
        }
    }

    pub fn finish(&self) {
        // Note for myself: we are in a single-threaded event loop, may not care about atomicity.
        self.finish.set(true);
    }

    pub async fn listen(
        &self,
        ws_provider: &RootProvider<PubSubFrontend, Ethereum>,
        http_provider: &HttpProvider,
    ) -> Result<()> {
        // Connections
        let (mut ps_stream, mut ef_stream, mut pv_stream) =
            connections::build_executor_subscriptions(&self.config, ws_provider).await?;

        let this_address = &self.config.public_key()?;
        let abi = connections::get_abi_from_path("/Users/sasha/dev/near-sffl/workers/abi/ArbitrumL0V2Endpoint.json")?;
        // Create a contract instance.
        let contract = contracts::create_contract_instance(&self.config, http_provider, abi)?;

        // Note: we expect to have a single element in this queue. Unfortunately, an author didn't
        // find a good single object holder in Rust (AtomicPtr/RefCell are too complex to handle)
        let mut packet_sent_queue: VecDeque<PacketSent> = VecDeque::default();

        // TODO: tokio::spawn(async move ...) ?
        while !&self.finish.get() {
            tokio::select! {
                Some(log) = ps_stream.next() => {
                    match log.log_decode::<PacketSent>() {
                        Ok(packet_sent) => {
                            debug!("PacketSent received");
                            packet_sent_queue.push_front(packet_sent.data().clone());
                        },
                        Err(e) => { error!("Failed to decode PacketSent event: {:?}", e);}
                    }
                },
                Some(log) = ef_stream.next() => {
                    match log.log_decode::<ExecutorFeePaid>() {
                        Ok(executor_fee_log) => {
                            debug!("ExecutorFeePaid received");
                            if packet_sent_queue.is_empty() {
                                continue;
                            }

                            if !this_address.eq(&executor_fee_log.data().executor)  {
                                packet_sent_queue.clear();
                                continue;
                            }
                            info!("Executor was chosen for execution!");
                        },
                        Err(e) => { error!("Failed to decode ExecutorFeePaid event: {:?}", e);}
                    }
                },
                Some(log) = pv_stream.next() => {
                    match log.log_decode::<PacketVerified>() {
                        Ok(inner_log) => {
                            debug!("PacketVerified received");
                            // Note: at the beginning executor may receive a couple of
                            // `PacketVerified` messages, but they will not be processed
                            // due to absence of previously received
                            let _ = Self::handle_verified_packet(
                                &contract,
                                &mut packet_sent_queue,
                                inner_log.data()
                            ).await;
                        },
                        Err(e) => { error!("Failed to decode PacketVerified event: {:?}", e);}
                    }
                },
                else => {
                    debug!("Nothing to handle.");
                    tokio::time::sleep(Duration::from_secs(1)).await;
                    continue;
                }
            }
        }
        Ok(())
    }

    async fn handle_verified_packet(
        contract: &ContractInst,
        queue: &mut VecDeque<PacketSent>,
        packet_verified: &PacketVerified,
    ) -> Result<()> {
        if queue.is_empty() {
            return Ok(());
        }
        let packet_sent = queue.pop_front().unwrap();
        // We don't expect any item to be present. If we have any - it is garbage.
        queue.clear();
        // Despite being described with other arguments, the only real implementation in
        // the contract of the `executable` function located here: https://shorturl.at/4H6Yz
        // function executable(Origin memory _origin, address _receiver) returns (ExecutionState)
        let call_builder = contract.function(
            "executable",
            &[
                Executor::prepare_header(&packet_verified.origin), // Origin (selected header fields)
                DynSolValue::Address(packet_verified.receiver),    // receiver address
            ],
        )?;

        let raw_packet = packet_sent.encodedPayload.iter().as_slice();
        loop {
            let call_result = call_builder.call().await?;
            match call_result[0] {
                DynSolValue::Int(I256::ZERO, 32) => {
                    // state: NotExecutable, continue to await commits
                    sleep(Duration::from_millis(500)).await;
                    continue;
                }
                DynSolValue::Int(I256::ONE, 32) => {
                    // state: Executable, firing lz_receive
                    Self::lz_receive(contract, raw_packet, packet_verified).await?;
                    break;
                }
                // We may ignore Executed status, it just frees the executor.
                _ => break,
            };
        }
        Ok(())
    }

    /// If the state is `Executable`, your `Executor` should decode the packet's options
    /// using the options.ts package and call the Endpoint's `lzReceive` function with
    /// the packet information:
    /// `endpoint.lzReceive(_origin, _receiver, _guid, _message, _extraData)`
    async fn lz_receive(
        contract: &ContractInst,
        raw_packet_encoded: &[u8],
        packet_verified: &PacketVerified,
    ) -> alloy::contract::Result<Vec<DynSolValue>> {
        // endpoint.lzReceive(_origin, _receiver, _guid, _message, _extraData)
        match Self::deserialize(raw_packet_encoded) {
            Some((guid, message)) => {
                contract
                    .function(
                        "lzReceive",
                        &[
                            Executor::prepare_header(&packet_verified.origin),
                            DynSolValue::Address(packet_verified.receiver),
                            DynSolValue::FixedBytes(guid, 32),
                            DynSolValue::Bytes(message),
                            DynSolValue::Bytes(vec![]), // TODO: no idea what is extra data for now...
                        ],
                    )?
                    .call()
                    .await
            }
            // Note: I didn't find any suitable error by type in alloy:Result.
            // Sending back just empty vec for now.
            None => Ok(vec![]),
        }
    }

    /// Converts `Origin` data structure from the received `PacketVerified`
    /// to the `DynSolValue`, understandable by `alloy-rs`.
    fn prepare_header(origin: &Origin) -> DynSolValue {
        CustomStruct {
            name: String::from("Origin"),
            prop_names: vec![String::from("srcEid"), String::from("sender"), String::from("nonce")],
            tuple: vec![
                DynSolValue::Uint(U256::from(origin.srcEid), 32),
                DynSolValue::Bytes(origin.sender.to_vec()),
                DynSolValue::Uint(U256::from(origin.nonce), 64),
            ],
        }
    }

    /// Extract `guid` and `message` from raw encoded packet.
    /// Note: this code is temporal and may be replaced with
    /// library-generated code in the future.
    fn deserialize(raw_packet: &[u8]) -> Option<(FixedBytes<32>, Vec<u8>)> {
        const MINIMUM_PACKET_LENGTH: usize = 93; // 1 + 8 + 4 + 32 + 4 + 32 + 32
        if raw_packet.len() < MINIMUM_PACKET_LENGTH {
            return None;
        }
        let mut buffered_packet = BytesMut::from(raw_packet);
        buffered_packet.advance(1); // version
        buffered_packet.get_u64(); // nonce
        buffered_packet.get_u32(); // src_eid
        buffered_packet.advance(32); // skip sender address, padded to 32 bytes.
        buffered_packet.get_u32(); // dst_eid
        buffered_packet.advance(32); // skip rcv address, padded to 32 bytes.
        let guid: FixedBytes<32> = FixedBytes::from_slice(buffered_packet.split_to(32).freeze().iter().as_slice());
        let message = buffered_packet.freeze().to_vec();

        Some((guid, message))
    }
}

#[cfg(test)]
mod test {
    use crate::executor_def::Executor;
    use alloy::primitives::FixedBytes;

    #[test]
    fn test_deserialize_happy_path() {
        const MESSAGE_OFFSET: usize = 43; // Just for this input
        const GUID_OFFSET: usize = MESSAGE_OFFSET + 32;
        let raw_packet_vec: Vec<u8> = vec![
            1, 0, 0, 0, 0, 0, 0, 17, 148, 0, 0, 117, 158, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 25, 207, 206, 71, 237,
            84, 168, 134, 20, 100, 141, 195, 241, 154, 89, 128, 9, 112, 7, 221, 0, 0, 118, 86, 0, 0, 0, 0, 0, 0, 0, 0,
            0, 0, 0, 0, 156, 45, 199, 55, 119, 23, 96, 62, 185, 43, 38, 85, 197, 242, 231, 153, 122, 73, 69, 189, 205,
            50, 72, 156, 47, 106, 85, 252, 73, 42, 176, 56, 154, 105, 225, 195, 98, 35, 62, 174, 103, 79, 244, 166,
            185, 78, 116, 114, 198, 247, 179, 202, 1, 0, 13, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 115, 135, 217, 64, 43,
            109, 214, 128, 114, 89, 78, 125, 198, 218, 97, 243, 152, 229, 200, 67, 0, 0, 0, 0, 0, 0, 0, 10,
        ];

        let left_guid_bound = raw_packet_vec.len() - GUID_OFFSET;
        let left_msg_bound = raw_packet_vec.len() - MESSAGE_OFFSET;

        let expected_guid_arr: &[u8] = &raw_packet_vec.as_slice()[left_guid_bound..left_msg_bound];
        let expected_message: &[u8] = &raw_packet_vec.as_slice()[left_msg_bound..];

        let raw_packet = raw_packet_vec.as_slice();
        let (guid, message) = Executor::deserialize(raw_packet).unwrap();
        let expected_guid: FixedBytes<32> = FixedBytes::from_slice(expected_guid_arr);
        assert_eq!(guid, expected_guid);
        assert_eq!(message.as_slice(), expected_message);
    }
}
