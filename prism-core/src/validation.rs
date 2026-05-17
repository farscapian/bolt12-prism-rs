use regex::Regex;
use std::sync::OnceLock;

use crate::types::{Destination, FeesIncurredBy, PrismError};

// ── Compiled regexes (compiled once, reused) ───────────────────────────────

static PUBKEY_REGEX: OnceLock<Regex> = OnceLock::new();
static BOLT12_REGEX: OnceLock<Regex> = OnceLock::new();

fn pubkey_regex() -> &'static Regex {
    PUBKEY_REGEX.get_or_init(|| {
        Regex::new(r"^0[2-3][0-9a-fA-F]{64}$").expect("valid pubkey regex")
    })
}

fn bolt12_regex() -> &'static Regex {
    BOLT12_REGEX.get_or_init(|| {
        Regex::new(r"^ln([a-zA-Z0-9]{1,90})[0-9]+[munp]?[a-zA-Z0-9]+[0-9]+[munp]?[a-zA-Z0-9]*$")
            .expect("valid bolt12 regex")
    })
}

// ── Destination parsing ────────────────────────────────────────────────────

/// Parse a raw destination string into a typed Destination.
/// Note: Bolt12Offer variants require an additional async decode() call
/// against the node to confirm validity — see NodeInterface.
pub fn parse_destination(raw: &str) -> Result<Destination, PrismError> {
    if raw.is_empty() {
        return Ok(Destination::Empty);
    }
    if pubkey_regex().is_match(raw) {
        return Ok(Destination::NodePubkey(raw.to_string()));
    }
    if bolt12_regex().is_match(raw) {
        return Ok(Destination::Bolt12Offer(raw.to_string()));
    }
    Err(PrismError::Validation(format!(
        "destination '{}' is not a valid BOLT12 offer, node pubkey, or empty string",
        raw
    )))
}

// ── Member field validation ────────────────────────────────────────────────

pub fn validate_member_description(description: &str) -> Result<(), PrismError> {
    if description.is_empty() {
        return Err(PrismError::Validation(
            "member description must not be empty".into(),
        ));
    }
    Ok(())
}

pub fn validate_member_split(split: f64) -> Result<(), PrismError> {
    if split <= 0.0 {
        return Err(PrismError::Validation(
            "member split must be a positive number".into(),
        ));
    }
    Ok(())
}

pub fn validate_fees_incurred_by(raw: &str) -> Result<FeesIncurredBy, PrismError> {
    match raw {
        "local" => Ok(FeesIncurredBy::Local),
        "remote" => Ok(FeesIncurredBy::Remote),
        other => Err(PrismError::Validation(format!(
            "fees_incurred_by must be 'local' or 'remote', got '{}'",
            other
        ))),
    }
}

// ── Prism field validation ─────────────────────────────────────────────────

pub fn validate_prism_description(description: &str) -> Result<(), PrismError> {
    if description.is_empty() {
        return Err(PrismError::Validation(
            "prism description must not be empty".into(),
        ));
    }
    Ok(())
}

pub fn validate_outlay_factor(factor: f64) -> Result<(), PrismError> {
    if factor <= 0.0 || factor > 10.0 {
        return Err(PrismError::Validation(format!(
            "outlay_factor must be between 0.0 (exclusive) and 10.0 (inclusive), got {}",
            factor
        )));
    }
    Ok(())
}

pub fn validate_member_count(count: usize) -> Result<(), PrismError> {
    if count == 0 {
        return Err(PrismError::Validation(
            "a prism must have at least one member".into(),
        ));
    }
    Ok(())
}