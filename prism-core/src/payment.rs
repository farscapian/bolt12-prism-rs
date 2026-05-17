use std::collections::HashMap;
use crate::node::{NodeInterface, PaymentStatus};
use crate::split::{apply_outlay_factor, calculate_member_payouts, new_outlay_after_payment};
use crate::types::{Destination, Prism, PrismBinding, PrismError};

/// Result of a single prism payout execution.
/// None means the member was skipped (empty destination, below threshold, etc.)
pub type PayoutResults = HashMap<String, Option<MemberPayoutResult>>;

#[derive(Debug, Clone)]
pub struct MemberPayoutResult {
    pub member_id: String,
    pub amount_msat: u64,
    pub amount_sent_msat: u64,
    pub payment_hash: String,
    pub status: PaymentStatus,
}

/// Execute a prism payout without a binding (e.g. prism-pay RPC).
/// Distributes total_msat * outlay_factor proportionally among members.
/// Does not respect payout_threshold_msat — that only applies to bindings.
pub async fn execute_prism_pay(
    prism: &Prism,
    amount_msat: u64,
    node: &dyn NodeInterface,
) -> Result<PayoutResults, PrismError> {
    let total_outlays = apply_outlay_factor(amount_msat, prism.outlay_factor);
    let payouts = calculate_member_payouts(&prism.members, total_outlays);

    let mut results = PayoutResults::new();

    for (member_id, member_msat) in payouts {
        let member = prism
            .members
            .iter()
            .find(|m| m.id == member_id)
            .ok_or_else(|| PrismError::NotFound(member_id.clone()))?;

        let result = pay_member(node, member_id.clone(), &member.destination, member_msat).await;

        match result {
            Ok(payout) => { results.insert(member_id, Some(payout)); }
            Err(e) => {
                // Log and continue — a single member failure does not abort the prism
                // prism-cln will log this via plugin.log(); core just skips
                eprintln!("payment skipped for member {}: {}", member_id, e);
                results.insert(member_id, None);
            }
        }
    }

    Ok(results)
}

/// Execute a prism payout triggered by a binding (e.g. invoice_payment hook).
/// Increments outlays first, then pays members whose outlay exceeds their threshold.
/// Updates the binding's outlays in place after each successful payment.
pub async fn execute_binding_pay(
    prism: &Prism,
    binding: &mut PrismBinding,
    amount_msat: u64,
    node: &dyn NodeInterface,
) -> Result<PayoutResults, PrismError> {
    // 1. Apply outlay factor
    let total_outlays = apply_outlay_factor(amount_msat, prism.outlay_factor);

    // 2. Increment each member's outlay accumulator
    increment_outlays(prism, binding, total_outlays)?;

    let mut results = PayoutResults::new();

    // 3. Pay each member whose accumulated outlay exceeds their threshold
    for member in &prism.members {
        let current_outlay = *binding.outlays.get(&member.id).unwrap_or(&0);

        if current_outlay <= member.payout_threshold_msat as i64 {
            // Below threshold — accumulate and defer
            results.insert(member.id.clone(), None);
            continue;
        }

        let member_msat = current_outlay as u64;
        let result = pay_member(node, member.id.clone(), &member.destination, member_msat).await;

        match result {
            Ok(payout) if payout.status == PaymentStatus::Complete => {
                // 4. Update outlay to reflect what was actually spent
                let new_outlay = new_outlay_after_payment(
                    current_outlay,
                    &member.fees_incurred_by,
                    payout.amount_msat,
                    payout.amount_sent_msat,
                );
                binding.outlays.insert(member.id.clone(), new_outlay);
                results.insert(member.id.clone(), Some(payout));
            }
            Ok(payout) => {
                // Payment returned but was not complete — leave outlay unchanged
                eprintln!("payment incomplete for member {}", member.id);
                results.insert(member.id.clone(), Some(payout));
            }
            Err(e) => {
                // Payment failed — leave outlay unchanged so it retries next time
                eprintln!("payment failed for member {}: {}", member.id, e);
                results.insert(member.id.clone(), None);
            }
        }
    }

    Ok(results)
}

/// Increment each member's outlay accumulator by their proportional share
/// of total_outlays, based on split weights.
fn increment_outlays(
    prism: &Prism,
    binding: &mut PrismBinding,
    total_outlays: u64,
) -> Result<(), PrismError> {
    let payouts = calculate_member_payouts(&prism.members, total_outlays);

    for (member_id, member_msat) in payouts {
        if !binding.outlays.contains_key(&member_id) {
            return Err(PrismError::Validation(format!(
                "binding and prism out of sync: member {} not found in binding outlays",
                member_id
            )));
        }
        *binding.outlays.entry(member_id).or_insert(0) += member_msat as i64;
    }

    Ok(())
}

/// Route a single payment to a member based on their destination type.
async fn pay_member(
    node: &dyn NodeInterface,
    member_id: String,
    destination: &Destination,
    amount_msat: u64,
) -> Result<MemberPayoutResult, PrismError> {
    match destination {
        Destination::Bolt12Offer(offer) => {
            let payment = node.fetch_and_pay_bolt12(offer, amount_msat).await?;
            Ok(MemberPayoutResult {
                member_id,
                amount_msat: payment.amount_msat,
                amount_sent_msat: payment.amount_sent_msat,
                payment_hash: payment.payment_hash,
                status: payment.status,
            })
        }
        Destination::NodePubkey(pubkey) => {
            let payment = node.keysend(pubkey, amount_msat).await?;
            Ok(MemberPayoutResult {
                member_id,
                amount_msat: payment.amount_msat,
                amount_sent_msat: payment.amount_sent_msat,
                payment_hash: payment.payment_hash,
                status: payment.status,
            })
        }
        Destination::Empty => Err(PrismError::Validation(format!(
            "member {} has no destination set; skipping",
            member_id
        ))),
    }
}