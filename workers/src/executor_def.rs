use crate::abi::L0V2EndpointAbi::{PacketSent, PacketVerified};
use crate::abi::ReceiveLibraryAbi::PayloadVerified;
use crate::abi::SendLibraryAbi::ExecutorFeePaid;
use crate::config::{DVNConfig, LayerZeroEvent};
use alloy::dyn_abi::DynSolValue;
use alloy::primitives::{keccak256};
use alloy::sol_types::SolEvent;
use eyre::Result;
use futures::StreamExt;
use std::collections::VecDeque;
use tracing::{error, info};
use crate::chain::{connections, contracts};

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
        // may not care about atomicity (yet?).
        self.finish = true
    }

    pub async fn listen<T: SolEvent>(&self) -> Result<()> {
        let (mut ps_stream, mut ef_stream, mut pv_stream) = connections::build_executor_subscriptions(&self.config).await?;

        let http_provider = connections::get_http_provider(&self.config)?;
        let l0_abi = connections::get_abi_from_path("./abi/ArbitrumL0V2Endpoint.json")?;
        let s_uln302 = connections::get_abi_from_path("./abi/ArbitrumSendLibUln302.json")?;
        let r_uln302 = connections::get_abi_from_path("./abi/ArbitrumReceiveLibUln302.json")?;
        // Create a contract instance.
        let contract = contracts::create_contract_instance(&self.config, http_provider, r_uln302)?;

        let mut packet_sent_queue: VecDeque<PacketSent> = VecDeque::default();

        // TODO: tokio::spawn(async move ...) ?
        while !&self.finish {
            tokio::select! {
                Some(log) = ps_stream.next() => {
                    match log.log_decode::<PacketSent>() {
                        Ok(packet_log) => {
                            packet_sent_queue.push_back(packet_log.data().clone());
                        },
                        Err(e) => { error!("Failed to decode PacketSent event: {:?}", e);}
                    }
                }
                Some(log) = ef_stream.next() => {
                    match log.log_decode::<ExecutorFeePaid>() {
                        Ok(executor_fee_log) => {
                           if packet_sent_queue.is_empty() {
                                continue;
                            }
                            if !executor_fee_log.data().executor.eq(&self.config.dvn_addr()?)  {
                                packet_sent_queue.clear();
                                continue;
                            }
                        },
                        Err(e) => { error!("Failed to decode ExecutorFeePaid event: {:?}", e);}
                    }
                },
                    Some(log) = pv_stream.next() => {
                    match log.log_decode::<PayloadVerified>() {
                        Ok(inner_log) => {
                            let payload_verified: &PayloadVerified = inner_log.data();
                            while let Some(packet) = packet_sent_queue.pop_back() {
                                let call_builder = contract.function(
                                    "_executable",
                                    &[
                                        DynSolValue::Bytes(payload_verified.header.to_vec()),
                                        DynSolValue::Bytes(keccak256(packet.encodedPayload).to_vec()),
                                    ],
                                )?;

                                let call_result = call_builder.call().await?;

                                println!("Execution state: {:?}", call_result);
                            }
                        },
                        Err(e) => { error!("Failed to decode PacketVerified event: {:?}", e);}
                    }
                },
            }
        }
        Ok(())
    }
}
