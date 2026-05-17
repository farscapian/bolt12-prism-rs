use prism_core::node::NodeInterface;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use cln_plugin::Plugin;
use serde_json::json;
use sha2::{Digest, Sha256};
use tokio::sync::Mutex;
use uuid::Uuid;

use cln_rpc::ClnRpc;
use prism_core::types::{Destination, FeesIncurredBy, Member, Prism, PrismBinding, PrismError};
use prism_core::validation::{
    parse_destination, validate_fees_incurred_by, validate_member_count,
    validate_member_description, validate_member_split, validate_outlay_factor,
    validate_prism_description,
};

use crate::store;

// ── Shared state passed to every handler ──────────────────────────────────

pub type RpcState = Arc<Mutex<ClnRpc>>;

// ── Helpers ────────────────────────────────────────────────────────────────

fn now_timestamp() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

fn generate_id() -> String {
    let mut hasher = Sha256::new();
    hasher.update(Uuid::new_v4().to_string());
    format!("{:x}", hasher.finalize())
}

/// Parse a member dict from a serde_json::Value into a Member struct.
fn parse_member(v: &serde_json::Value, preserve_id: bool) -> Result<Member, PrismError> {
    let description = v["description"]
        .as_str()
        .ok_or_else(|| PrismError::Validation("member missing 'description'".into()))?
        .to_string();

    let destination_str = v["destination"]
        .as_str()
        .ok_or_else(|| PrismError::Validation("member missing 'destination'".into()))?;

    let split = v["split"]
        .as_f64()
        .ok_or_else(|| PrismError::Validation("member missing or invalid 'split'".into()))?;

    let fees_str = v["fees_incurred_by"].as_str().unwrap_or("remote");
    let payout_threshold_msat = v["payout_threshold_msat"].as_u64().unwrap_or(0);

    validate_member_description(&description)?;
    validate_member_split(split)?;
    let fees_incurred_by = validate_fees_incurred_by(fees_str)?;
    let destination = parse_destination(destination_str)?;

    let id = if preserve_id {
        v["member_id"]
            .as_str()
            .ok_or_else(|| PrismError::Validation("member missing 'member_id' (required for update)".into()))?
            .to_string()
    } else {
        v["member_id"]
            .as_str()
            .map(|s| s.to_string())
            .unwrap_or_else(generate_id)
    };

    Ok(Member {
        id,
        description,
        destination,
        split,
        fees_incurred_by,
        payout_threshold_msat,
    })
}

fn member_to_json(m: &Member) -> serde_json::Value {
    let destination_str = match &m.destination {
        Destination::Bolt12Offer(s) => s.clone(),
        Destination::NodePubkey(s) => s.clone(),
        Destination::Empty => "".to_string(),
    };
    let fees_str = match m.fees_incurred_by {
        FeesIncurredBy::Local => "local",
        FeesIncurredBy::Remote => "remote",
    };
    json!({
        "member_id": m.id,
        "description": m.description,
        "destination": destination_str,
        "split": m.split,
        "fees_incurred_by": fees_str,
        "payout_threshold_msat": m.payout_threshold_msat,
    })
}

fn prism_to_json(p: &Prism) -> serde_json::Value {
    json!({
        "prism_id": p.id,
        "description": p.description,
        "timestamp": p.timestamp,
        "outlay_factor": p.outlay_factor,
        "prism_members": p.members.iter().map(member_to_json).collect::<Vec<_>>(),
    })
}

fn binding_to_json(b: &PrismBinding) -> serde_json::Value {
    json!({
        "offer_id": b.offer_id,
        "prism_id": b.prism_id,
        "timestamp": b.timestamp,
        "member_outlays": b.outlays.iter().map(|(id, outlay)| json!({
            "member_id": id,
            "outlay_msat": outlay,
        })).collect::<Vec<_>>(),
    })
}

// ── RPC handlers ───────────────────────────────────────────────────────────

pub async fn prism_create(
    plugin: Plugin<RpcState>,
    v: serde_json::Value,
) -> Result<serde_json::Value, anyhow::Error> {
    let description = v["description"].as_str().unwrap_or("").to_string();
    let outlay_factor = v["outlay_factor"].as_f64().unwrap_or(1.0);

    validate_prism_description(&description).map_err(anyhow::Error::msg)?;
    validate_outlay_factor(outlay_factor).map_err(anyhow::Error::msg)?;

    let members_raw = v["members"]
        .as_array()
        .ok_or_else(|| anyhow::anyhow!("'members' must be an array"))?;

    validate_member_count(members_raw.len()).map_err(anyhow::Error::msg)?;

    let members: Vec<Member> = members_raw
        .iter()
        .map(|m| parse_member(m, false))
        .collect::<Result<_, _>>()
        .map_err(anyhow::Error::msg)?;

    let prism = Prism {
        id: generate_id(),
        description,
        timestamp: now_timestamp(),
        outlay_factor,
        members,
    };

    let state = plugin.state().clone();
    let mut rpc = state.lock().await;
    store::save_prism(&mut rpc, &prism).await.map_err(anyhow::Error::msg)?;

    Ok(prism_to_json(&prism))
}

pub async fn prism_list(
    plugin: Plugin<RpcState>,
    v: serde_json::Value,
) -> Result<serde_json::Value, anyhow::Error> {
    let state = plugin.state().clone();
    let mut rpc = state.lock().await;

    if let Some(prism_id) = v["prism_id"].as_str() {
        let prism = store::load_prism(&mut rpc, prism_id)
            .await
            .map_err(anyhow::Error::msg)?;
        return Ok(json!({ "prisms": [prism_to_json(&prism)] }));
    }

    let ids = store::list_prism_ids(&mut rpc).await.map_err(anyhow::Error::msg)?;
    let mut prisms = Vec::new();
    for id in ids {
        let prism = store::load_prism(&mut rpc, &id).await.map_err(anyhow::Error::msg)?;
        prisms.push(prism_to_json(&prism));
    }

    Ok(json!({ "prisms": prisms }))
}

pub async fn prism_update(
    plugin: Plugin<RpcState>,
    v: serde_json::Value,
) -> Result<serde_json::Value, anyhow::Error> {
    let prism_id = v["prism_id"]
        .as_str()
        .ok_or_else(|| anyhow::anyhow!("missing 'prism_id'"))?;

    let members_raw = v["members"]
        .as_array()
        .ok_or_else(|| anyhow::anyhow!("'members' must be an array"))?;

    validate_member_count(members_raw.len()).map_err(anyhow::Error::msg)?;

    // preserve_id=true: member_id required in each member dict
    let members: Vec<Member> = members_raw
        .iter()
        .map(|m| parse_member(m, true))
        .collect::<Result<_, _>>()
        .map_err(anyhow::Error::msg)?;

    let state = plugin.state().clone();
    let mut rpc = state.lock().await;

    let mut prism = store::load_prism(&mut rpc, prism_id)
        .await
        .map_err(anyhow::Error::msg)?;

    prism.members = members;
    store::save_prism(&mut rpc, &prism).await.map_err(anyhow::Error::msg)?;

    Ok(prism_to_json(&prism))
}

pub async fn prism_delete(
    plugin: Plugin<RpcState>,
    v: serde_json::Value,
) -> Result<serde_json::Value, anyhow::Error> {
    let prism_id = v["prism_id"]
        .as_str()
        .ok_or_else(|| anyhow::anyhow!("missing 'prism_id'"))?;

    let state = plugin.state().clone();
    let mut rpc = state.lock().await;

    let prism = store::load_prism(&mut rpc, prism_id)
        .await
        .map_err(anyhow::Error::msg)?;

    // Refuse deletion if bindings exist
    let bindings = store::list_bindings(&mut rpc).await.map_err(anyhow::Error::msg)?;
    let bound = bindings.iter().any(|b| b.prism_id == prism_id);
    if bound {
        return Err(anyhow::anyhow!(
            "prism '{}' has existing bindings; remove them first with prism-deletebinding",
            prism_id
        ));
    }

    store::delete_prism(&mut rpc, &prism).await.map_err(anyhow::Error::msg)?;

    Ok(json!({ "deleted": prism_id }))
}

pub async fn prism_pay(
    plugin: Plugin<RpcState>,
    v: serde_json::Value,
) -> Result<serde_json::Value, anyhow::Error> {
    let prism_id = v["prism_id"]
        .as_str()
        .ok_or_else(|| anyhow::anyhow!("missing 'prism_id'"))?;

    let amount_msat = v["amount_msat"]
        .as_u64()
        .ok_or_else(|| anyhow::anyhow!("missing or invalid 'amount_msat'"))?;

    if amount_msat == 0 {
        return Err(anyhow::anyhow!("amount_msat must be greater than 0"));
    }

    let state = plugin.state().clone();
    let mut rpc = state.lock().await;

    let prism = store::load_prism(&mut rpc, prism_id)
        .await
        .map_err(anyhow::Error::msg)?;

    // Build a ClnNode and execute the payout
    let node = crate::node::ClnNode::new(plugin.state().clone());
    let results = prism_core::payment::execute_prism_pay(&prism, amount_msat, &node)
        .await
        .map_err(anyhow::Error::msg)?;

    let payout_json: HashMap<String, serde_json::Value> = results
        .into_iter()
        .map(|(member_id, result)| {
            let val = match result {
                Some(r) => json!({
                    "payment_hash": r.payment_hash,
                    "amount_msat": r.amount_msat,
                    "amount_sent_msat": r.amount_sent_msat,
                    "status": format!("{:?}", r.status).to_lowercase(),
                }),
                None => json!(null),
            };
            (member_id, val)
        })
        .collect();

    Ok(json!({ "prism_member_payouts": payout_json }))
}

pub async fn prism_addbinding(
    plugin: Plugin<RpcState>,
    v: serde_json::Value,
) -> Result<serde_json::Value, anyhow::Error> {
    let prism_id = v["prism_id"]
        .as_str()
        .ok_or_else(|| anyhow::anyhow!("missing 'prism_id'"))?;

    let offer_id = v["offer_id"]
        .as_str()
        .ok_or_else(|| anyhow::anyhow!("missing 'offer_id'"))?;

    let state = plugin.state().clone();
    let mut rpc = state.lock().await;

    // Validate offer exists on the node
    let node = crate::node::ClnNode::new(plugin.state().clone());
    let exists = node.offer_exists(offer_id).await.map_err(anyhow::Error::msg)?;
    if !exists {
        return Err(anyhow::anyhow!("offer '{}' does not exist on this node", offer_id));
    }

    let prism = store::load_prism(&mut rpc, prism_id)
        .await
        .map_err(anyhow::Error::msg)?;

    let outlays: HashMap<String, i64> = prism.members.iter().map(|m| (m.id.clone(), 0)).collect();
    let timestamp = now_timestamp();

    let binding = PrismBinding {
        offer_id: offer_id.to_string(),
        prism_id: prism_id.to_string(),
        timestamp,
        outlays,
    };

    // Use create_binding (must-create); if already exists, fall back to save_binding (must-replace)
    let result = store::create_binding(&mut rpc, &binding).await;
    if result.is_err() {
        store::save_binding(&mut rpc, &binding).await.map_err(anyhow::Error::msg)?;
    }

    Ok(json!({
        "offer_id": offer_id,
        "prism_id": prism_id,
        "timestamp": timestamp,
        "prism_members": prism.members.iter().map(member_to_json).collect::<Vec<_>>(),
    }))
}

pub async fn prism_listbindings(
    plugin: Plugin<RpcState>,
    v: serde_json::Value,
) -> Result<serde_json::Value, anyhow::Error> {
    let state = plugin.state().clone();
    let mut rpc = state.lock().await;

    if let Some(offer_id) = v["offer_id"].as_str() {
        let binding = store::load_binding(&mut rpc, offer_id)
            .await
            .map_err(anyhow::Error::msg)?;
        // Fixed: always return array, not bare dict (bug in Python version)
        return Ok(json!({ "bolt12_prism_bindings": [binding_to_json(&binding)] }));
    }

    let bindings = store::list_bindings(&mut rpc).await.map_err(anyhow::Error::msg)?;
    Ok(json!({
        "bolt12_prism_bindings": bindings.iter().map(binding_to_json).collect::<Vec<_>>()
    }))
}

pub async fn prism_deletebinding(
    plugin: Plugin<RpcState>,
    v: serde_json::Value,
) -> Result<serde_json::Value, anyhow::Error> {
    let offer_id = v["offer_id"]
        .as_str()
        .ok_or_else(|| anyhow::anyhow!("missing 'offer_id'"))?;

    let state = plugin.state().clone();
    let mut rpc = state.lock().await;

    // Confirm it exists before deleting
    store::load_binding(&mut rpc, offer_id)
        .await
        .map_err(anyhow::Error::msg)?;

    store::delete_binding(&mut rpc, offer_id)
        .await
        .map_err(anyhow::Error::msg)?;

    Ok(json!({ "binding_removed": true }))
}

pub async fn prism_setoutlay(
    plugin: Plugin<RpcState>,
    v: serde_json::Value,
) -> Result<serde_json::Value, anyhow::Error> {
    let offer_id = v["offer_id"]
        .as_str()
        .ok_or_else(|| anyhow::anyhow!("missing 'offer_id'"))?;

    let member_id = v["member_id"]
        .as_str()
        .ok_or_else(|| anyhow::anyhow!("missing 'member_id'"))?;

    let new_outlay_msat = v["new_outlay_msat"].as_i64().unwrap_or(0);

    let state = plugin.state().clone();
    let mut rpc = state.lock().await;

    let mut binding = store::load_binding(&mut rpc, offer_id)
        .await
        .map_err(anyhow::Error::msg)?;

    if !binding.outlays.contains_key(member_id) {
        return Err(anyhow::anyhow!("member '{}' not found in binding", member_id));
    }

    binding.outlays.insert(member_id.to_string(), new_outlay_msat);
    store::save_binding(&mut rpc, &binding).await.map_err(anyhow::Error::msg)?;

    Ok(json!({ "bolt12_prism_bindings": binding_to_json(&binding) }))
}