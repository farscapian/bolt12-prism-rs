use async_trait::async_trait;
use crate::types::PrismError;

/// Result of a successful payment leg.
#[derive(Debug, Clone)]
pub struct PaymentResult {
    pub payment_hash: String,
    pub amount_msat: u64,       // amount received by destination (excluding fees)
    pub amount_sent_msat: u64,  // total spent by our node (including fees)
    pub status: PaymentStatus,
}

#[derive(Debug, Clone, PartialEq)]
pub enum PaymentStatus {
    Complete,
    Failed,
    Pending,
}

/// Abstraction over Lightning node I/O.
/// prism-core depends only on this trait.
/// prism-cln implements it against CLN's RPC.
/// A future prism-lnd would implement it against LND's gRPC.
#[async_trait]
pub trait NodeInterface: Send + Sync {
    /// Fetch a BOLT12 invoice from the remote peer and pay it.
    async fn fetch_and_pay_bolt12(
        &self,
        offer: &str,
        amount_msat: u64,
    ) -> Result<PaymentResult, PrismError>;

    /// Send a keysend payment to a node pubkey.
    async fn keysend(
        &self,
        pubkey: &str,
        amount_msat: u64,
    ) -> Result<PaymentResult, PrismError>;

    /// Validate a BOLT12 offer string against the node's decoder.
    /// Returns true if the node considers it a valid, active offer.
    async fn decode_offer(&self, offer: &str) -> Result<bool, PrismError>;

    /// Check whether an offer with the given offer_id exists on this node.
    async fn offer_exists(&self, offer_id: &str) -> Result<bool, PrismError>;
}