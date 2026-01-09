pub mod chain;
pub mod eureka_monitor;
#[cfg(feature = "grpc")]
pub mod grpc;
pub mod priority;
pub mod service;

// Re-export key types from chain module
pub use chain::{
    ChainClient as ChainClientTrait, ChainClientPool, ChainError, ClientState, Coin,
    ConsensusState, CosmosChainClient, CosmosMsg, Height, MerkleProof, TxBuilder, TxResponse,
};

// Re-export from priority module
pub use priority::{PrioritizedPacket, PriorityLevel, PriorityQueue, RetryInfo};

// Re-export from service module (excluding duplicate ChainClient and ChainConfig)
pub use service::{
    ChainConfig, MockChainClient, PacketDetails, PacketProof, RelayerConfig, RelayerError,
    SolverRelayer,
};

// Re-export from eureka_monitor module
pub use eureka_monitor::{EurekaPacketMonitor, MockEurekaMonitor};

#[cfg(feature = "grpc")]
pub use grpc::GrpcChainClient;
