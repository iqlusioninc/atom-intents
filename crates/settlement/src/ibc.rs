use atom_intents_types::IbcTransferInfo;
use cosmwasm_std::Uint128;
use serde::{Deserialize, Serialize};

/// IBC flow type for settlement
#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum IbcFlowType {
    /// Same chain transfer (~3s)
    SameChain,

    /// Direct IBC transfer (~6s)
    DirectIbc { channel: String },

    /// Multi-hop via Packet Forward Middleware (~15-20s)
    MultiHopPfm { hops: Vec<PfmHop> },

    /// IBC Hooks with Wasm execution (~10-15s)
    IbcHooksWasm { contract: String, msg: String },
}

/// A hop in PFM routing
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PfmHop {
    pub receiver: String,
    pub channel: String,
}

/// Build PFM memo for multi-hop transfers
pub fn build_pfm_memo(hops: &[PfmHop]) -> String {
    if hops.is_empty() {
        return String::new();
    }

    fn build_nested(hops: &[PfmHop]) -> serde_json::Value {
        if hops.is_empty() {
            return serde_json::Value::Null;
        }

        let hop = &hops[0];
        let mut obj = serde_json::json!({
            "forward": {
                "receiver": hop.receiver,
                "channel": hop.channel
            }
        });

        if hops.len() > 1 {
            if let Some(next) = build_nested(&hops[1..]).as_object() {
                obj["forward"]["next"] = serde_json::Value::Object(next.clone());
            }
        }

        obj
    }

    build_nested(hops).to_string()
}

/// Build IBC hooks memo for Wasm execution
pub fn build_wasm_memo(contract: &str, msg: &serde_json::Value, forward: Option<&PfmHop>) -> String {
    let mut memo = serde_json::json!({
        "wasm": {
            "contract": contract,
            "msg": msg
        }
    });

    if let Some(hop) = forward {
        memo["forward"] = serde_json::json!({
            "receiver": hop.receiver,
            "channel": hop.channel
        });
    }

    memo.to_string()
}

/// Calculate appropriate IBC timeout based on flow type
pub fn calculate_timeout(flow_type: &IbcFlowType, base_timeout_secs: u64) -> u64 {
    let multiplier = match flow_type {
        IbcFlowType::SameChain => 1,
        IbcFlowType::DirectIbc { .. } => 2,
        IbcFlowType::MultiHopPfm { hops } => 2 + hops.len() as u64,
        IbcFlowType::IbcHooksWasm { .. } => 3,
    };

    base_timeout_secs * multiplier
}

/// Determine the best IBC flow for a transfer
pub fn determine_flow(
    source_chain: &str,
    dest_chain: &str,
    needs_swap: bool,
    channel_map: &std::collections::HashMap<(String, String), String>,
) -> IbcFlowType {
    // Same chain
    if source_chain == dest_chain {
        return IbcFlowType::SameChain;
    }

    // Direct channel exists
    let key = (source_chain.to_string(), dest_chain.to_string());
    if let Some(channel) = channel_map.get(&key) {
        if needs_swap {
            // Would need IBC hooks for swap
            return IbcFlowType::IbcHooksWasm {
                contract: "swap_router".to_string(),
                msg: "{}".to_string(),
            };
        }
        return IbcFlowType::DirectIbc {
            channel: channel.clone(),
        };
    }

    // Need multi-hop
    // In production, this would query a routing table
    IbcFlowType::MultiHopPfm { hops: vec![] }
}

/// IBC transfer builder
pub struct IbcTransferBuilder {
    source_chain: String,
    dest_chain: String,
    channel: String,
    denom: String,
    amount: Uint128,
    sender: String,
    receiver: String,
    timeout_secs: u64,
    memo: Option<String>,
}

impl IbcTransferBuilder {
    pub fn new(
        source_chain: impl Into<String>,
        dest_chain: impl Into<String>,
        channel: impl Into<String>,
    ) -> Self {
        Self {
            source_chain: source_chain.into(),
            dest_chain: dest_chain.into(),
            channel: channel.into(),
            denom: String::new(),
            amount: Uint128::zero(),
            sender: String::new(),
            receiver: String::new(),
            timeout_secs: 600, // 10 minutes default
            memo: None,
        }
    }

    pub fn denom(mut self, denom: impl Into<String>) -> Self {
        self.denom = denom.into();
        self
    }

    pub fn amount(mut self, amount: Uint128) -> Self {
        self.amount = amount;
        self
    }

    pub fn sender(mut self, sender: impl Into<String>) -> Self {
        self.sender = sender.into();
        self
    }

    pub fn receiver(mut self, receiver: impl Into<String>) -> Self {
        self.receiver = receiver.into();
        self
    }

    pub fn timeout_secs(mut self, secs: u64) -> Self {
        self.timeout_secs = secs;
        self
    }

    pub fn memo(mut self, memo: impl Into<String>) -> Self {
        self.memo = Some(memo.into());
        self
    }

    pub fn build(self, current_time: u64) -> IbcTransferInfo {
        IbcTransferInfo {
            source_chain: self.source_chain,
            dest_chain: self.dest_chain,
            channel: self.channel,
            amount: self.amount,
            denom: self.denom,
            sender: self.sender,
            receiver: self.receiver,
            timeout_timestamp: current_time + self.timeout_secs,
            memo: self.memo,
        }
    }
}
