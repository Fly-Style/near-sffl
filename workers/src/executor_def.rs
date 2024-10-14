use crate::abi::L0V2EndpointAbi::{Origin, PacketSent, PacketVerified};
use crate::abi::SendLibraryAbi::ExecutorFeePaid;
use crate::chain::{connections, contracts, ContractInst, HttpProvider};
use crate::config::DVNConfig;
use alloy::dyn_abi::{DynSolValue, DynSolValue::CustomStruct};
use alloy::network::Ethereum;
use alloy::primitives::bytes::{Buf, Bytes, BytesMut};
use alloy::primitives::{keccak256, FixedBytes, I256, U256};
use alloy::providers::RootProvider;
use alloy::pubsub::PubSubFrontend;
use eyre::Result;
use futures::StreamExt;
use std::cell::Cell;
use std::collections::VecDeque;
use log::info;
use tokio::time::{sleep, Duration};
use tracing::{debug, error, warn};

pub struct Executor {
    config: DVNConfig,
    finish: Cell<bool>,
}

// For test purposes
pub enum ExecutorPacketStatusTracker {
    Initial,
    PacketSent,
    FeePaid,
    Verified,
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
        let call_builder = contract.function(
            "executable",
            &[
                Executor::prepare_header(&packet_verified.origin),
                DynSolValue::Address(packet_verified.receiver)
                // DynSolValue::Bytes(keccak256(packet_sent.encodedPayload).to_vec()),
            ],
        )?;

        loop {
            let call_result = call_builder.call().await?;
            match call_result[0] {
                DynSolValue::Int(I256::ZERO, 32) => {
                    // NotExecutable, continue to await commits
                    sleep(Duration::from_millis(500)).await;
                    continue;
                }
                DynSolValue::Int(I256::ONE, 32) => {
                    // Executable
                    Self::lz_receive(contract, packet_verified).await?;
                    break;
                }
                // We may ignore Executed status, it just free the executor.
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
        packet_verified: &PacketVerified,
    ) -> alloy::contract::Result<Vec<DynSolValue>> {
        // endpoint.lzReceive(_origin, _receiver, _guid, _message, _extraData)
        contract
            .function(
                "lzReceive",
                &[
                    Executor::prepare_header(&packet_verified.origin),
                    DynSolValue::Address(packet_verified.receiver),
                    // DynSolValue::FixedBytes(packet.guid, 32),
                    // DynSolValue::Bytes(packet.message.to_vec()),
                    DynSolValue::Bytes(vec![]),
                ],
            )?
            .call()
            .await
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
}
