use std::collections::HashMap;
use serde::{Deserialize, Serialize};

// ── Enums ─────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum FeesIncurredBy {
    Local,
    Remote,
}

impl Default for FeesIncurredBy {
    fn default() -> Self {
        FeesIncurredBy::Remote
    }
}

/// Represents where a prism member receives their payment.
/// Validated at construction time so downstream code never sees a raw string.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", content = "value", rename_all = "snake_case")]
pub enum Destination {
    Bolt12Offer(String),
    NodePubkey(String),
    Empty,
}

// ── Member ─────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Member {
    pub id: String,
    pub description: String,
    pub destination: Destination,
    pub split: f64,
    pub fees_incurred_by: FeesIncurredBy,
    pub payout_threshold_msat: u64,
}

// ── Prism ──────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Prism {
    pub id: String,
    pub description: String,
    pub timestamp: u64,
    pub outlay_factor: f64,
    pub members: Vec<Member>,
}

impl Prism {
    /// Sum of all member splits. Used as the denominator in payout calculation.
    pub fn total_splits(&self) -> f64 {
        self.members.iter().map(|m| m.split).sum()
    }
}

// ── PrismBinding ───────────────────────────────────────────────────────────

/// Binds a prism to a BOLT12 offer. Tracks per-member payment outlays.
/// Outlays are signed: negative means the member owes the host.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PrismBinding {
    pub offer_id: String,
    pub prism_id: String,
    pub timestamp: u64,
    pub outlays: HashMap<String, i64>,
}

// ── Errors ─────────────────────────────────────────────────────────────────

#[derive(Debug, thiserror::Error)]
pub enum PrismError {
    #[error("Prism not found: {0}")]
    NotFound(String),

    #[error("Binding not found for offer: {0}")]
    BindingNotFound(String),

    #[error("Validation error: {0}")]
    Validation(String),

    #[error("Payment failed for member {member_id}: {reason}")]
    PaymentFailed { member_id: String, reason: String },

    #[error("Node RPC error: {0}")]
    NodeRpc(String),

    #[error("Serialization error: {0}")]
    Serialization(#[from] serde_json::Error),
}