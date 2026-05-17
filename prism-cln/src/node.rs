use async_trait::async_trait;
use cln_rpc::{ClnRpc, Request, Response};
use cln_rpc::model::requests::{FetchinvoiceRequest, PayRequest, KeysendRequest, DecodeRequest, ListoffersRequest};
use prism_core::node::{NodeInterface, PaymentResult, PaymentStatus};
use prism_core::types::PrismError;
use tokio::sync::Mutex;
use std::sync::Arc;

/// CLN implementation of NodeInterface.
/// Wraps a ClnRpc handle behind an Arc<Mutex<>> for shared async access.
pub struct ClnNode {
    rpc: Arc<Mutex<ClnRpc>>,
}

impl ClnNode {
    pub fn new(rpc: Arc<Mutex<ClnRpc>>) -> Self {
        Self { rpc }
    }
}

#[async_trait]
impl NodeInterface for ClnNode {
    async fn fetch_and_pay_bolt12(
        &self,
        offer: &str,
        amount_msat: u64,
    ) -> Result<PaymentResult, PrismError> {
        let mut rpc = self.rpc.lock().await;

        // Step 1: fetch invoice from remote peer
        let fetch_req = FetchinvoiceRequest {
            offer: offer.to_string(),
            amount_msat: Some(cln_rpc::primitives::Amount::from_msat(amount_msat)),
            quantity: None,
            recurrence_counter: None,
            recurrence_start: None,
            recurrence_label: None,
            timeout: None,
            payer_note: None,
        };

        let Response::FetchInvoice(fetch_res) = rpc
            .call(Request::FetchInvoice(fetch_req))
            .await
            .map_err(|e| PrismError::NodeRpc(format!("fetchinvoice failed: {}", e)))?
        else {
            return Err(PrismError::NodeRpc("unexpected response from fetchinvoice".into()));
        };

        // Step 2: pay the fetched invoice
        let pay_req = PayRequest {
            bolt11: fetch_res.invoice,
            amount_msat: None,
            label: None,
            riskfactor: None,
            maxfeepercent: None,
            retry_for: None,
            maxdelay: None,
            exemptfee: None,
            localinvreqid: None,
            exclude: None,
            maxfee: None,
            description: None,
            partial_msat: None,
        };

        let Response::Pay(pay_res) = rpc
            .call(Request::Pay(pay_req))
            .await
            .map_err(|e| PrismError::NodeRpc(format!("pay failed: {}", e)))?
        else {
            return Err(PrismError::NodeRpc("unexpected response from pay".into()));
        };

        Ok(PaymentResult {
            payment_hash: pay_res.payment_hash.to_string(),
            amount_msat: pay_res.amount_msat.msat(),
            amount_sent_msat: pay_res.amount_sent_msat.msat(),
            status: PaymentStatus::Complete,
        })
    }

    async fn keysend(
        &self,
        pubkey: &str,
        amount_msat: u64,
    ) -> Result<PaymentResult, PrismError> {
        let mut rpc = self.rpc.lock().await;

        let destination = pubkey
            .parse::<cln_rpc::primitives::PublicKey>()
            .map_err(|e| PrismError::Validation(format!("invalid pubkey: {}", e)))?;

        let req = KeysendRequest {
            destination,
            amount_msat: cln_rpc::primitives::Amount::from_msat(amount_msat),
            label: None,
            maxfeepercent: None,
            retry_for: None,
            maxdelay: None,
            exemptfee: None,
            routehints: None,
            extratlvs: None,
        };

        let Response::KeySend(res) = rpc
            .call(Request::KeySend(req))
            .await
            .map_err(|e| PrismError::NodeRpc(format!("keysend failed: {}", e)))?
        else {
            return Err(PrismError::NodeRpc("unexpected response from keysend".into()));
        };

        Ok(PaymentResult {
            payment_hash: res.payment_hash.to_string(),
            amount_msat: res.amount_msat.msat(),
            amount_sent_msat: res.amount_sent_msat.msat(),
            status: PaymentStatus::Complete,
        })
    }

    async fn decode_offer(&self, offer: &str) -> Result<bool, PrismError> {
        let mut rpc = self.rpc.lock().await;

        let req = DecodeRequest {
            string: offer.to_string(),
        };

        let Response::Decode(res) = rpc
            .call(Request::Decode(req))
            .await
            .map_err(|e| PrismError::NodeRpc(format!("decode failed: {}", e)))?
        else {
            return Err(PrismError::NodeRpc("unexpected response from decode".into()));
        };

        Ok(res.valid)
    }

    async fn offer_exists(&self, offer_id: &str) -> Result<bool, PrismError> {
        let mut rpc = self.rpc.lock().await;

        let req = ListoffersRequest {
            offer_id: Some(
                offer_id
                    .parse()
                    .map_err(|e| PrismError::Validation(format!("invalid offer_id: {}", e)))?
            ),
            active_only: None,
        };

        let Response::ListOffers(res) = rpc
            .call(Request::ListOffers(req))
            .await
            .map_err(|e| PrismError::NodeRpc(format!("listoffers failed: {}", e)))?
        else {
            return Err(PrismError::NodeRpc("unexpected response from listoffers".into()));
        };

        Ok(!res.offers.is_empty())
    }
}