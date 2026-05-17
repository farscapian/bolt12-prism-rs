mod node;
mod rpc;
mod store;

use anyhow::anyhow;
use cln_plugin::Builder;
use cln_rpc::ClnRpc;
use std::sync::Arc;
use tokio::sync::Mutex;

use rpc::RpcState;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Connect to CLN's RPC socket
    let rpc_path = std::env::var("CLN_PLUGIN_LOG_LEVEL")
        .ok()
        .and_then(|_| std::env::var("LIGHTNINGD_RPC_FILE").ok())
        .unwrap_or_else(|| "lightning-rpc".to_string());

    let rpc = ClnRpc::new(&rpc_path)
        .await
        .map_err(|e| anyhow!("failed to connect to CLN RPC: {}", e))?;

    let state: RpcState = Arc::new(Mutex::new(rpc));

    let configured_plugin = Builder::new(tokio::io::stdin(), tokio::io::stdout())
        .rpcmethod("prism-create", "Create a prism", rpc::prism_create)
        .rpcmethod("prism-list", "List prisms", rpc::prism_list)
        .rpcmethod(
            "prism-update",
            "Update an existing prism",
            rpc::prism_update,
        )
        .rpcmethod("prism-delete", "Delete a prism", rpc::prism_delete)
        .rpcmethod("prism-pay", "Execute a prism payout", rpc::prism_pay)
        .rpcmethod(
            "prism-addbinding",
            "Bind a prism to a BOLT12 offer",
            rpc::prism_addbinding,
        )
        .rpcmethod(
            "prism-listbindings",
            "List prism bindings",
            rpc::prism_listbindings,
        )
        .rpcmethod(
            "prism-deletebinding",
            "Remove a prism binding",
            rpc::prism_deletebinding,
        )
        .rpcmethod(
            "prism-setoutlay",
            "Set a member outlay value for a binding",
            rpc::prism_setoutlay,
        )
        .subscribe("invoice_payment", on_payment)
        .configure()
        .await?
        .ok_or_else(|| anyhow!("plugin configuration failed"))?;

    configured_plugin.start(state).await?.join().await?;

    Ok(())
}

async fn on_payment(
    plugin: cln_plugin::Plugin<RpcState>,
    v: serde_json::Value,
) -> anyhow::Result<()> {
    let payment = &v["invoice_payment"];

    // Guard: only process invoices with a local_offer_id (BOLT12)
    // Fixes the Python bug where BOLT11 payments caused a NameError
    let offer_id = match payment["local_offer_id"].as_str() {
        Some(id) => id.to_string(),
        None => return Ok(()), // not a BOLT12 payment; skip silently
    };

    let amount_msat = match payment["msat"].as_u64() {
        Some(v) => v,
        None => {
            log::warn!("invoice_payment missing msat field");
            return Ok(());
        }
    };

    let state = plugin.state().clone();
    let mut rpc = state.lock().await;

    // Load the binding; if none exists this is not a prism payment
    let mut binding = match store::load_binding(&mut rpc, &offer_id).await {
        Ok(b) => b,
        Err(_) => return Ok(()), // no binding for this offer; skip
    };

    // Load the associated prism
    let prism = match store::load_prism(&mut rpc, &binding.prism_id).await {
        Ok(p) => p,
        Err(e) => {
            log::warn!("could not load prism for binding {}: {}", offer_id, e);
            return Ok(());
        }
    };

    // Release the rpc lock before making outbound payments
    drop(rpc);

    let node = node::ClnNode::new(plugin.state().clone());

    match prism_core::payment::execute_binding_pay(&prism, &mut binding, amount_msat, &node).await {
        Ok(_) => {
            // Persist updated outlays after payment
            let mut rpc = plugin.state().lock().await;
            if let Err(e) = store::save_binding(&mut rpc, &binding).await {
                log::warn!("failed to save binding after payment: {}", e);
            }
        }
        Err(e) => {
            log::warn!("execute_binding_pay failed for offer {}: {}", offer_id, e);
        }
    }

    Ok(())
}
