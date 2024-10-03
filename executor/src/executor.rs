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

use std::net::SocketAddr;
use rand::Rng;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

trait AbstractExecutor {
    //! Listen current network for LayerZero-specific events and handles them.
    fn listen(&self);

    fn idempotency_check(&self);
}