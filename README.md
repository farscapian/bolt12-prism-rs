# bolt12-prism-rs

A [Core Lightning (CLN)](https://github.com/ElementsProject/lightning) plugin that splits incoming BOLT12 payments among multiple recipients. Written in Rust.

A **prism** is a payment policy: when a BOLT12 offer on your node receives a payment, the plugin automatically distributes funds proportionally to a list of members — each with their own destination, split weight, and payout threshold.

## How it works

1. You create a **prism** defining a list of members and how to split payments among them.
2. You **bind** that prism to a BOLT12 offer on your node.
3. When that offer is paid, the plugin fires on the `invoice_payment` hook and distributes the funds.

Each member accumulates an **outlay** (a running balance). The plugin only sends a payment to that member once their outlay exceeds their `payout_threshold_msat`. This lets you batch small payments to save on routing fees.

## Data model

### Member fields

| Field | Type | Description |
|---|---|---|
| `description` | string | Human-readable label |
| `destination` | string | BOLT12 offer string (`lno...`) or node pubkey (66 hex chars), or `""` for empty |
| `split` | float | Relative share weight (e.g. `1.0` each = equal split) |
| `fees_incurred_by` | `"local"` \| `"remote"` | Who absorbs routing fees: host (`local`) or member (`remote`) |
| `payout_threshold_msat` | u64 | Minimum accumulated outlay before a payment is sent (0 = pay immediately) |

### Prism fields

| Field | Type | Description |
|---|---|---|
| `description` | string | Human-readable label |
| `outlay_factor` | float | Multiplier applied to the incoming amount before splitting (0 < x ≤ 10.0; use `1.0` to pass through 100%) |
| `members` | array | List of member objects |

### Fee policy

- `fees_incurred_by: "local"` — The host node absorbs routing fees; each member receives their full proportional share.
- `fees_incurred_by: "remote"` — Routing fees are deducted from the member's outlay accumulator after payment.

## RPC commands

All commands are available via `lightning-cli` once the plugin is loaded.

### Prism management

**`prism-create`** — Create a new prism.

```bash
lightning-cli prism-create \
  description="my prism" \
  outlay_factor=1.0 \
  members='[
    {"description":"Alice","destination":"lno1...","split":2.0,"fees_incurred_by":"local","payout_threshold_msat":1000000},
    {"description":"Bob","destination":"lno1...","split":1.0,"fees_incurred_by":"remote","payout_threshold_msat":0}
  ]'
```

**`prism-list`** — List all prisms, or fetch one by ID.

```bash
lightning-cli prism-list
lightning-cli prism-list prism_id=<id>
```

**`prism-update`** — Replace the member list on an existing prism. Each member dict must include its `member_id`.

```bash
lightning-cli prism-update prism_id=<id> members='[...]'
```

**`prism-delete`** — Delete a prism. Fails if active bindings exist (remove them first).

```bash
lightning-cli prism-delete prism_id=<id>
```

### Payments

**`prism-pay`** — Immediately pay out a prism without a binding. Does not respect `payout_threshold_msat`.

```bash
lightning-cli prism-pay prism_id=<id> amount_msat=100000
```

### Bindings

**`prism-addbinding`** — Bind a prism to a BOLT12 offer. The offer must exist on this node.

```bash
lightning-cli prism-addbinding prism_id=<id> offer_id=<offer_id>
```

**`prism-listbindings`** — List all bindings, or fetch one by offer ID.

```bash
lightning-cli prism-listbindings
lightning-cli prism-listbindings offer_id=<offer_id>
```

**`prism-deletebinding`** — Remove a binding.

```bash
lightning-cli prism-deletebinding offer_id=<offer_id>
```

**`prism-setoutlay`** — Manually set a member's outlay accumulator in a binding. Useful for correcting state after a failed payment or resetting thresholds.

```bash
lightning-cli prism-setoutlay offer_id=<offer_id> member_id=<member_id> new_outlay_msat=0
```

## Building

Requires Rust and a CLN node.

```bash
cargo build --release
```

The plugin binary is at `target/release/prism-cln`. Add it to your CLN config:

```
plugin=/path/to/prism-cln
```

## Integration test

`test.sh` spins up a 3-node regtest environment (alice, bob, carol) using Docker, builds the plugin, funds channels, and exercises the full prism-create → prism-pay → prism-addbinding flow.

Requirements: Docker, `docker compose`, `jq`.

```bash
./test.sh
```

To explore manually after the test:

```bash
docker compose -f cln-docker/docker-compose.yml exec cln1 lightning-cli --network=regtest getinfo
```

To tear down:

```bash
docker compose -f cln-docker/docker-compose.yml down -v
```

## Project structure

```
bolt12-prism-rs/
├── prism-core/        # Core types, payment logic, split math, validation (no CLN dependency)
│   └── src/
│       ├── types.rs       # Prism, Member, PrismBinding, Destination, FeesIncurredBy
│       ├── payment.rs     # execute_prism_pay, execute_binding_pay
│       ├── split.rs       # calculate_member_payouts, apply_outlay_factor
│       ├── validation.rs  # Input validation (regex for offers/pubkeys, business rules)
│       └── node.rs        # NodeInterface trait (abstraction over the LN node)
├── prism-cln/         # CLN plugin (wires prism-core to cln-plugin / cln-rpc)
│   └── src/
│       ├── main.rs        # Plugin entry, hook registration
│       ├── rpc.rs         # RPC handlers
│       ├── node.rs        # ClnNode: NodeInterface impl using cln-rpc
│       └── store.rs       # Persistence using CLN's datastore
└── cln-docker/        # Docker setup for local dev/testing
    ├── docker-compose.yml
    ├── Dockerfile.cln
    └── entrypoint.sh
```
