use crate::abi::L0V2EndpointAbi::{Origin, PacketSent, PacketVerified};
use crate::abi::SendLibraryAbi::ExecutorFeePaid;
use crate::chain::{connections, contracts, ContractInst, HttpProvider};
use crate::config::DVNConfig;
use alloy::dyn_abi::{DynSolValue, DynSolValue::CustomStruct};
use alloy::network::Ethereum;
use alloy::primitives::{keccak256, U256};
use alloy::providers::RootProvider;
use alloy::pubsub::PubSubFrontend;
use eyre::Result;
use futures::StreamExt;
use std::collections::VecDeque;
use tokio::time;
use tracing::{debug, error, warn};

pub struct Executor {
    config: DVNConfig,
    finish: bool,
}

pub enum ExecutorPacketStatusTracker {
    Initial,
    PacketSent,
    FeePaid,
    Verified,
    // ...
}

impl Executor {
    pub fn new(config: DVNConfig) -> Self {
        Executor { config, finish: false }
    }

    pub fn finish(&mut self) {
        // Note for myself: we are in a single-threaded event loop,
        // may not care about atomicity (yet).
        self.finish = true
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
        let abi = connections::get_abi_from_path("Users/sasha/dev/near-sffl/workers/abi/EndpointV2View.json")?;
        // Create a contract instance.
        let contract = contracts::create_contract_instance(&self.config, http_provider, abi)?;

        let mut packet_sent_queue: VecDeque<PacketSent> = VecDeque::default();

        // TODO: tokio::spawn(async move ...) ?
        while !&self.finish {
            tokio::select! {
                Some(log) = ps_stream.next() => {
                    match log.log_decode::<PacketSent>() {
                        Ok(packet_log) => {
                            debug!("PacketSent received");
                            packet_sent_queue.push_back(packet_log.data().clone());
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
                            warn!("Executor was chosen for execution!");
                        },
                        Err(e) => { error!("Failed to decode ExecutorFeePaid event: {:?}", e);}
                    }
                },
                Some(log) = pv_stream.next() => {
                    match log.log_decode::<PacketVerified>() {
                        Ok(inner_log) => {
                            debug!("PacketVerified received");
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
                    tokio::time::sleep(time::Duration::from_secs(3)).await;
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
        while let Some(packet) = queue.pop_back() {
            let call_builder = contract.function(
                "executable",
                &[
                    Executor::prepare_header(&packet_verified.origin),
                    DynSolValue::Bytes(keccak256(packet.encodedPayload).to_vec()),
                ],
            )?;

            // TODO: offload to tokio::spawn with resubmission logic.
            let call_result = call_builder.call().await;
            warn!("Execution state: {:?}", call_result);
        }
        Ok(())
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
