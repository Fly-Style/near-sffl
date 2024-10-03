use num256::Uint256;
use serde::{Deserialize, Serialize};

pub type SolidityAddress = [u8; 20];

// #[derive(Debug, Clone, Serialize, Deserialize)]
// pub struct PacketSent {
//     #[serde(rename = "encodedPacket")]
//     pub(crate) encoded_packet: Vec<u8>,
//     pub(crate) options: Vec<u8>,
//     #[serde(rename = "sendLibrary")]
//     pub(crate) send_library: SolidityAddress,
// }

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecutorPeeFaid {
    executor: SolidityAddress,
    fee: Uint256
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PayloadVerified {
    #[serde(rename = "dvnAddress")]
    dvn_address: SolidityAddress,
    header: Vec<u8>,
    confirmations: Uint256, // should be uint256
    #[serde(rename = "proofHash")]
    proof_hash: Vec<u8>,
}

pub enum VerificationState {
    Verifying,
    Verifiable,
    Verified,
}

pub enum ExecutionState {
    NotExecutable,
    Executable,
    Executed,
}