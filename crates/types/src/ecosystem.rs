use cosmwasm_schema::cw_serde;

/// Supported ecosystems for liquidity sourcing
#[cw_serde]
#[derive(Copy, Hash, Eq)]
pub enum Ecosystem {
    Cosmos,
    Ethereum,
    Near,
}

impl Ecosystem {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Cosmos => "cosmos",
            Self::Ethereum => "ethereum",
            Self::Near => "near",
        }
    }
}

/// Direction of Eureka transfer
#[cw_serde]
#[derive(Copy, Hash, Eq)]
pub enum EurekaDirection {
    CosmosToEthereum,
    EthereumToCosmos,
}

/// Direction of Omnibridge transfer (for future NEAR integration)
#[cw_serde]
#[derive(Copy, Hash, Eq)]
pub enum OmnibridgeDirection {
    CosmosToNear,
    NearToCosmos,
}

/// Settlement path describing how assets reach the user
#[cw_serde]
pub enum SettlementPath {
    /// Cosmos IBC Classic
    CosmosIbc { channel: String, is_multi_hop: bool },

    /// Ethereum via IBC Eureka
    Eureka {
        direction: EurekaDirection,
        eth_address: Option<String>,
    },

    /// NEAR via Omnibridge (future)
    NearOmnibridge {
        direction: OmnibridgeDirection,
        near_account: Option<String>,
    },
}

impl SettlementPath {
    pub fn ecosystem(&self) -> Ecosystem {
        match self {
            Self::CosmosIbc { .. } => Ecosystem::Cosmos,
            Self::Eureka { .. } => Ecosystem::Ethereum,
            Self::NearOmnibridge { .. } => Ecosystem::Near,
        }
    }

    pub fn estimated_time_secs(&self) -> u64 {
        match self {
            Self::CosmosIbc { is_multi_hop, .. } => {
                if *is_multi_hop {
                    20
                } else {
                    6
                }
            }
            Self::Eureka { .. } => 25,
            Self::NearOmnibridge { .. } => 45,
        }
    }

    pub fn estimated_cost_usd(&self) -> f64 {
        match self {
            Self::CosmosIbc { is_multi_hop, .. } => {
                if *is_multi_hop {
                    0.02
                } else {
                    0.01
                }
            }
            Self::Eureka { .. } => 3.0,
            Self::NearOmnibridge { .. } => 0.10,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ecosystem_display() {
        assert_eq!(Ecosystem::Cosmos.as_str(), "cosmos");
        assert_eq!(Ecosystem::Ethereum.as_str(), "ethereum");
        assert_eq!(Ecosystem::Near.as_str(), "near");
    }

    #[test]
    fn test_settlement_path_cosmos_ibc() {
        let path = SettlementPath::CosmosIbc {
            channel: "channel-141".to_string(),
            is_multi_hop: false,
        };

        assert_eq!(path.ecosystem(), Ecosystem::Cosmos);
        assert_eq!(path.estimated_time_secs(), 6);
    }

    #[test]
    fn test_settlement_path_eureka() {
        let path = SettlementPath::Eureka {
            direction: EurekaDirection::EthereumToCosmos,
            eth_address: Some("0x123".to_string()),
        };

        assert_eq!(path.ecosystem(), Ecosystem::Ethereum);
        assert!(path.estimated_time_secs() >= 15);
    }
}
