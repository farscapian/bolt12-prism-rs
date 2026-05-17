use std::collections::HashMap;
use cln_rpc::{ClnRpc, Request, Response};
use cln_rpc::model::requests::{ListdatastoreRequest, DatastoreRequest, DeldatastoreRequest};
use cln_rpc::model::requests::DatastoreMode;
use prism_core::types::{Member, Prism, PrismBinding, PrismError};

const DB_VERSION: &str = "v2.1";

// ── Key builders ───────────────────────────────────────────────────────────

pub fn member_key(id: &str) -> Vec<String> {
    vec!["prism".into(), DB_VERSION.into(), "member".into(), id.into()]
}

pub fn prism_key(id: &str) -> Vec<String> {
    vec!["prism".into(), DB_VERSION.into(), "prism".into(), id.into()]
}

pub fn binding_key(offer_id: &str) -> Vec<String> {
    vec!["prism".into(), DB_VERSION.into(), "bind".into(), "bolt12".into(), offer_id.into()]
}

pub fn all_prisms_key() -> Vec<String> {
    vec!["prism".into(), DB_VERSION.into(), "prism".into()]
}

pub fn all_bindings_key() -> Vec<String> {
    vec!["prism".into(), DB_VERSION.into(), "bind".into(), "bolt12".into()]
}

// ── Raw datastore helpers ──────────────────────────────────────────────────

pub async fn datastore_get(rpc: &mut ClnRpc, key: Vec<String>) -> Result<Option<String>, PrismError> {
    let req = ListdatastoreRequest { key: Some(key) };
    let Response::ListDatastore(res) = rpc
        .call(Request::ListDatastore(req))
        .await
        .map_err(|e| PrismError::NodeRpc(e.to_string()))?
    else {
        return Err(PrismError::NodeRpc("unexpected response type".into()));
    };

    Ok(res.datastore.into_iter().next().and_then(|r| r.string))
}

pub async fn datastore_list(rpc: &mut ClnRpc, key: Vec<String>) -> Result<Vec<(Vec<String>, String)>, PrismError> {
    let req = ListdatastoreRequest { key: Some(key) };
    let Response::ListDatastore(res) = rpc
        .call(Request::ListDatastore(req))
        .await
        .map_err(|e| PrismError::NodeRpc(e.to_string()))?
    else {
        return Err(PrismError::NodeRpc("unexpected response type".into()));
    };

    Ok(res
        .datastore
        .into_iter()
        .filter_map(|r| r.string.map(|s| (r.key, s)))
        .collect())
}

pub async fn datastore_put(
    rpc: &mut ClnRpc,
    key: Vec<String>,
    value: String,
    mode: DatastoreMode,
) -> Result<(), PrismError> {
    let req = DatastoreRequest {
        key,
        string: Some(value),
        mode: Some(mode),
        hex: None,
        generation: None,
    };
    rpc.call(Request::Datastore(req))
        .await
        .map_err(|e| PrismError::NodeRpc(e.to_string()))?;
    Ok(())
}

pub async fn datastore_del(rpc: &mut ClnRpc, key: Vec<String>) -> Result<(), PrismError> {
    let req = DeldatastoreRequest { key, generation: None };
    rpc.call(Request::DelDatastore(req))
        .await
        .map_err(|e| PrismError::NodeRpc(e.to_string()))?;
    Ok(())
}

// ── Member persistence ─────────────────────────────────────────────────────

pub async fn save_member(rpc: &mut ClnRpc, member: &Member) -> Result<(), PrismError> {
    let json = serde_json::to_string(member)?;
    datastore_put(rpc, member_key(&member.id), json, DatastoreMode::CREATE_OR_REPLACE).await
}

pub async fn load_member(rpc: &mut ClnRpc, member_id: &str) -> Result<Member, PrismError> {
    let raw = datastore_get(rpc, member_key(member_id))
        .await?
        .ok_or_else(|| PrismError::NotFound(format!("member {}", member_id)))?;
    serde_json::from_str(&raw).map_err(PrismError::Serialization)
}

pub async fn delete_member(rpc: &mut ClnRpc, member_id: &str) -> Result<(), PrismError> {
    datastore_del(rpc, member_key(member_id)).await
}

// ── Prism persistence ──────────────────────────────────────────────────────

/// Prism records store member_ids only; members are stored separately.
#[derive(serde::Serialize, serde::Deserialize)]
struct PrismRecord {
    id: String,
    description: String,
    timestamp: u64,
    outlay_factor: f64,
    member_ids: Vec<String>,
}

pub async fn save_prism(rpc: &mut ClnRpc, prism: &Prism) -> Result<(), PrismError> {
    // Save each member separately
    for member in &prism.members {
        save_member(rpc, member).await?;
    }

    // Save prism record with member_ids only
    let record = PrismRecord {
        id: prism.id.clone(),
        description: prism.description.clone(),
        timestamp: prism.timestamp,
        outlay_factor: prism.outlay_factor,
        member_ids: prism.members.iter().map(|m| m.id.clone()).collect(),
    };
    let json = serde_json::to_string(&record)?;
    datastore_put(rpc, prism_key(&prism.id), json, DatastoreMode::CREATE_OR_REPLACE).await
}

pub async fn load_prism(rpc: &mut ClnRpc, prism_id: &str) -> Result<Prism, PrismError> {
    let raw = datastore_get(rpc, prism_key(prism_id))
        .await?
        .ok_or_else(|| PrismError::NotFound(format!("prism {}", prism_id)))?;

    let record: PrismRecord =
        serde_json::from_str(&raw).map_err(PrismError::Serialization)?;

    // Hydrate members from their individual records
    let mut members = Vec::new();
    for id in &record.member_ids {
        members.push(load_member(rpc, id).await?);
    }

    Ok(Prism {
        id: record.id,
        description: record.description,
        timestamp: record.timestamp,
        outlay_factor: record.outlay_factor,
        members,
    })
}

pub async fn list_prism_ids(rpc: &mut ClnRpc) -> Result<Vec<String>, PrismError> {
    let records = datastore_list(rpc, all_prisms_key()).await?;
    Ok(records
        .into_iter()
        .map(|(key, _)| key.into_iter().last().unwrap_or_default())
        .collect())
}

pub async fn delete_prism(rpc: &mut ClnRpc, prism: &Prism) -> Result<(), PrismError> {
    for member in &prism.members {
        delete_member(rpc, &member.id).await?;
    }
    datastore_del(rpc, prism_key(&prism.id)).await
}

// ── Binding persistence ────────────────────────────────────────────────────

#[derive(serde::Serialize, serde::Deserialize)]
struct BindingRecord {
    prism_id: String,
    timestamp: u64,
    member_outlays: HashMap<String, i64>,
}

pub async fn save_binding(rpc: &mut ClnRpc, binding: &PrismBinding) -> Result<(), PrismError> {
    let record = BindingRecord {
        prism_id: binding.prism_id.clone(),
        timestamp: binding.timestamp,
        member_outlays: binding.outlays.clone(),
    };
    let json = serde_json::to_string(&record)?;
    datastore_put(rpc, binding_key(&binding.offer_id), json, DatastoreMode::MUST_REPLACE).await
}

pub async fn create_binding(rpc: &mut ClnRpc, binding: &PrismBinding) -> Result<(), PrismError> {
    let record = BindingRecord {
        prism_id: binding.prism_id.clone(),
        timestamp: binding.timestamp,
        member_outlays: binding.outlays.clone(),
    };
    let json = serde_json::to_string(&record)?;
    datastore_put(rpc, binding_key(&binding.offer_id), json, DatastoreMode::MUST_CREATE).await
}

pub async fn load_binding(rpc: &mut ClnRpc, offer_id: &str) -> Result<PrismBinding, PrismError> {
    let raw = datastore_get(rpc, binding_key(offer_id))
        .await?
        .ok_or_else(|| PrismError::BindingNotFound(offer_id.to_string()))?;

    let record: BindingRecord =
        serde_json::from_str(&raw).map_err(PrismError::Serialization)?;

    Ok(PrismBinding {
        offer_id: offer_id.to_string(),
        prism_id: record.prism_id,
        timestamp: record.timestamp,
        outlays: record.member_outlays,
    })
}

pub async fn list_bindings(rpc: &mut ClnRpc) -> Result<Vec<PrismBinding>, PrismError> {
    let records = datastore_list(rpc, all_bindings_key()).await?;
    let mut bindings = Vec::new();
    for (key, raw) in records {
        let offer_id = key.into_iter().last().unwrap_or_default();
        let record: BindingRecord =
            serde_json::from_str(&raw).map_err(PrismError::Serialization)?;
        bindings.push(PrismBinding {
            offer_id,
            prism_id: record.prism_id,
            timestamp: record.timestamp,
            outlays: record.member_outlays,
        });
    }
    Ok(bindings)
}

pub async fn delete_binding(rpc: &mut ClnRpc, offer_id: &str) -> Result<(), PrismError> {
    datastore_del(rpc, binding_key(offer_id)).await
}