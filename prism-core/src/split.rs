use crate::types::{FeesIncurredBy, Member};

/// Calculate each member's payout in millisatoshis given a total outlay amount.
/// Uses integer arithmetic throughout to avoid floating-point rounding errors.
/// Splits are proportional weights, not percentages.
///
/// Example: three members with splits [1.0, 1.0, 2.0] and total_msat=1000
///   → member 0: floor(1000 * 1/4) = 250
///   → member 1: floor(1000 * 1/4) = 250
///   → member 2: floor(1000 * 2/4) = 500
pub fn calculate_member_payouts(members: &[Member], total_msat: u64) -> Vec<(String, u64)> {
    let total_splits: f64 = members.iter().map(|m| m.split).sum();

    members
        .iter()
        .map(|m| {
            let proportion = m.split / total_splits;
            let amount = (total_msat as f64 * proportion).floor() as u64;
            (m.id.clone(), amount)
        })
        .collect()
}

/// Apply the outlay_factor multiplier to an incoming amount.
/// Floors to the nearest millisatoshi.
///
///   factor < 1.0 → host retains a portion
///   factor = 1.0 → full pass-through
///   factor > 1.0 → host subsidizes (matching funds)
pub fn apply_outlay_factor(amount_msat: u64, outlay_factor: f64) -> u64 {
    (amount_msat as f64 * outlay_factor).floor() as u64
}

/// Calculate the new outlay value after a successful payment,
/// respecting the member's fee policy.
///
/// fees_incurred_by=Remote: fees deducted from member's share (member bears cost)
/// fees_incurred_by=Local:  fees paid by host (member receives full split amount)
pub fn new_outlay_after_payment(
    current_outlay: i64,
    fees_incurred_by: &FeesIncurredBy,
    amount_msat: u64,       // amount received by destination (excluding fees)
    amount_sent_msat: u64,  // total spent by our node (including fees)
) -> i64 {
    let deduction = match fees_incurred_by {
        FeesIncurredBy::Remote => amount_sent_msat as i64,
        FeesIncurredBy::Local => amount_msat as i64,
    };
    current_outlay - deduction
}

// ── Tests ──────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{Destination, FeesIncurredBy, Member};

    fn make_member(id: &str, split: f64) -> Member {
        Member {
            id: id.to_string(),
            description: "test".to_string(),
            destination: Destination::Empty,
            split,
            fees_incurred_by: FeesIncurredBy::Remote,
            payout_threshold_msat: 0,
        }
    }

    #[test]
    fn equal_splits_three_members() {
        let members = vec![
            make_member("a", 1.0),
            make_member("b", 1.0),
            make_member("c", 1.0),
        ];
        let payouts = calculate_member_payouts(&members, 1000);
        assert_eq!(payouts[0].1, 333);
        assert_eq!(payouts[1].1, 333);
        assert_eq!(payouts[2].1, 333);
    }

    #[test]
    fn weighted_splits() {
        let members = vec![
            make_member("a", 1.0),
            make_member("b", 1.0),
            make_member("c", 2.0),
        ];
        let payouts = calculate_member_payouts(&members, 1000);
        assert_eq!(payouts[0].1, 250);
        assert_eq!(payouts[1].1, 250);
        assert_eq!(payouts[2].1, 500);
    }

    #[test]
    fn outlay_factor_below_one() {
        assert_eq!(apply_outlay_factor(1000, 0.75), 750);
    }

    #[test]
    fn outlay_factor_above_one() {
        assert_eq!(apply_outlay_factor(1000, 1.1), 1100);
    }

    #[test]
    fn fee_accounting_remote() {
        // member bears fees: deduct amount_sent (which includes fees)
        let result = new_outlay_after_payment(1000, &FeesIncurredBy::Remote, 990, 1005);
        assert_eq!(result, -5); // 1000 - 1005
    }

    #[test]
    fn fee_accounting_local() {
        // host bears fees: deduct only amount received
        let result = new_outlay_after_payment(1000, &FeesIncurredBy::Local, 990, 1005);
        assert_eq!(result, 10); // 1000 - 990
    }
}