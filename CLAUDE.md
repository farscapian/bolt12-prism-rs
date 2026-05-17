# CLAUDE.md

## Build and test

```bash
cargo build              # debug build (used by test.sh)
cargo build --release    # production build
cargo test               # unit tests (split math, validation)
./test.sh                # full integration test — requires Docker and jq
```

The plugin binary path: `target/debug/prism-cln` (debug) or `target/release/prism-cln` (release).  
`test.sh` always does a `cargo build` (debug) first, then mounts that binary into Docker.

## Workspace layout

Two crates:

- **`prism-core`** — Pure library, no CLN dependency. Contains all business logic. Can be tested without a running node.
- **`prism-cln`** — CLN plugin binary. Wires `prism-core` to `cln-plugin` (RPC registration, hooks) and `cln-rpc` (node calls).

The `NodeInterface` trait in `prism-core/src/node.rs` is the seam between the two. `prism-cln/src/node.rs` implements it with real CLN RPC calls; in tests you can implement it with stubs.

## Persistence

CLN's built-in datastore is used (no external DB). Keys follow a hierarchical scheme:

- Prism metadata: `["prism", "v2.1", "prism", <prism_id>]`
- Prism members: `["prism", "v2.1", "member", <member_id>]`
- Bindings: `["prism", "v2.1", "binding", <offer_id>]`

Members are stored as separate records (not embedded in the prism JSON). When loading a prism, `store.rs` loads the prism record first (which contains `member_ids`), then fetches each member individually. This avoids hitting CLN datastore size limits on large prisms.

`DatastoreMode::MUST_CREATE` is used for new records; `MUST_REPLACE` for updates. This gives optimistic-concurrency semantics for free.

## Payment flow

**`prism-pay` RPC (no binding):**
1. `apply_outlay_factor(amount_msat, outlay_factor)` → `total_outlays`
2. `calculate_member_payouts(members, total_outlays)` → per-member msats (proportional, floor division)
3. Pay each member immediately via BOLT12 `fetchinvoice`+`pay` or keysend
4. `payout_threshold_msat` is ignored — all members are paid regardless

**`invoice_payment` hook (binding):**
1. Guard: only fires on BOLT12 payments (must have `local_offer_id`)
2. Load binding by `offer_id`; load associated prism
3. `apply_outlay_factor` → increment each member's outlay accumulator
4. For each member: only pay if `outlay > payout_threshold_msat`
5. On successful payment: update outlay with `new_outlay_after_payment` (fee policy applied here)
6. On failed payment: outlay is left unchanged so it retries on the next invoice

## Outlay semantics

`PrismBinding.outlays` is a `HashMap<member_id, i64>` (signed millisatoshis). Positive = member is owed money. The value starts at 0 when a binding is created and accumulates with each incoming payment.

After a successful payment:
- `fees_incurred_by: "local"` → outlay is decremented by `amount_msat` (what was targeted), not `amount_sent_msat` (which includes fees). Host eats the fee difference.
- `fees_incurred_by: "remote"` → outlay is decremented by `amount_sent_msat`. Member bears routing fees.

`prism-setoutlay` lets you manually override any member's outlay — useful for operational corrections.

## Input validation

All validation is in `prism-core/src/validation.rs`:
- BOLT12 offers: must start with `lno` (case-insensitive via regex)
- Node pubkeys: exactly 66 lowercase hex characters
- `outlay_factor`: `0 < x ≤ 10.0`
- `split`: must be positive (> 0)
- At least one member required per prism
- Non-empty `description` on both prisms and members

Empty string destination is allowed and stored as `Destination::Empty`. Paying a member with an empty destination is a no-op (skipped with a log message, not an error).

## Key design decisions

- **Single-failure isolation**: In both `execute_prism_pay` and `execute_binding_pay`, a payment failure for one member does not abort the others. Errors are logged and `None` is returned for that member.
- **No rounding budget**: Split math uses floor division throughout. Remainder sats are not distributed — they stay with the host. This is intentional and avoids complex remainder logic.
- **Prism-delete safety**: `prism-delete` refuses if any bindings reference the prism. You must `prism-deletebinding` first.
- **`prism-listbindings` always returns an array**: A known bug in the original Python version returned a bare dict for single-binding lookups. The Rust version always wraps in `bolt12_prism_bindings: [...]`.
- **`prism-update` requires `member_id`**: When updating members, each member object must include its existing `member_id`. This preserves outlay state in any bindings (which key by member_id). Omitting it would orphan accumulated outlays.
