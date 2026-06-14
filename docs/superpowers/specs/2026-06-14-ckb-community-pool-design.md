# CKB Community Pool — Design Spec

**Date:** 2026-06-14
**Status:** Design approved, pre-implementation
**Author:** Phill + Claude (brainstormed)

A federated, **non-custodial** PPLNS mining pool for CKB (Eaglesong PoW), community-owned and operated. Built by extending the existing `ckb-stratum-proxy` (Node.js, Eaglesong, K7/GodMiner support, vardiff, dashboard) rather than forking yiimp — yiimp is single-operator PHP/MySQL with no Eaglesong path and a muddy trust model. The new parts on top of the proxy are: **share accounting**, a **custom on-chain payout lock**, and a **federation mesh**.

---

## 1. Goals & non-goals

**Goals**
- No human/operator can custody or steal pool funds. Found blocks are unstealable by construction.
- Community-operated: many independent node-runners host localized, low-latency getwork endpoints that link into one pool.
- Minimal **1%** fee, split **50/50** between a pool trust (community treasury) and node operators.
- Operators rewarded for reliable infrastructure (uptime), not for happening to attract traffic.
- Miners paid fairly for contributed work via difficulty-weighted PPLNS, with payouts that respect CKB's hard cell-capacity floor.

**Non-goals (v1)**
- Permissionless operator membership / staking / slashing (future).
- PPS or any payout scheme requiring a custodied capital buffer.
- A P2Pool-style share chain / multi-output cellbase (requires patched CKB nodes; research-grade, out of scope).
- Multi-coin support.

---

## 2. Core decisions (locked)

| Area | Decision |
|------|----------|
| **Custody model** | Custom **on-chain payout lock** (trust-minimized). Cellbase → treasury cell → settlement tx, all under one lock. |
| **Accounting plane** | **Central aggregator + verify-before-sign.** Aggregator is untrusted for correctness; operators independently recompute and only then co-sign. |
| **Operator set** | **Fixed founding 3-of-5**, pubkeys committed in lock args. Membership changes = explicit governed rotation (new lock version + treasury migration). |
| **Accrual state** | **On-chain**, committed as a Merkle root in the treasury cell data. |
| **Payout threshold** | **100 CKB** per miner (just above the ~61 CKB cell floor). Sub-threshold balances carry forward in the accrual root. |
| **Fee** | **1%** of mined reward, **script-capped**. 50% → trust address; 50% → operators. |
| **Operator fee split** | By **peer-measured uptime** (passed health probes confirming it served valid pool-locked work). NOT share-weighted. |
| **Payout scheme** | **PPLNS**, difficulty-weighted. Window multiplier **W operator-configurable, default medium (~2× difficulty reference)**. PROP noted as simpler alternative if hopping is judged a non-threat. |
| **Control-plane transport** | **Tailscale / Headscale (WireGuard)** overlay. Self-hosted Headscale keeps the coordinator community-owned. |
| **Work-plane transport** | Public Stratum TCP (miners hit nearest operator; not part of the mesh). |
| **Repo** | New monorepo at `~/community-pool`, vendoring `ckb-stratum-proxy`. |

---

## 3. Architecture

Two planes:

```
                          WORK PLANE (public TCP, low-latency getwork)
   miners ──stratum──▶ ┌──────────────┐   miners ──stratum──▶ ┌──────────────┐
   (nearest)           │  Operator A  │   (nearest)           │  Operator B  │
                       │  ckb node ───┤                       │  ckb node ───┤
                       │  pool-proxy  │                       │  pool-proxy  │
                       └──────┬───────┘                       └──────┬───────┘
   block_assembler = POOL LOCK  │  (same lock on every node)            │
 ─────────────────────────────┼───────────────────────────────────────┼──────
   CONTROL PLANE (Tailscale/WireGuard — encrypted, NAT-traversed)       │
                              ▼                                         ▼
                       signed share-batches ───▶ ┌─────────────────────────┐
                       signed uptime probes  ───▶│   Aggregator (coord)    │
                                                 │  PPLNS ledger + root    │
                       operators recompute root  │  uptime vector          │
                       and M-of-N co-sign  ◀─────┤  settlement tx builder  │
                                                 └────────────┬────────────┘
                                                              ▼
                                            CKB L1: cellbase → treasury cell
                                                    → settlement tx (custom lock)
                                                    → miner/operator/trust payouts
```

**Foundational invariant:** every operator's node sets `block_assembler` to the *same* pool lock, so any block found anywhere pays the pool lock, and the winning PoW nonce is cryptographically bound to that exact block. Therefore the **work plane is custody-trustless on its own** — a malicious operator cannot redirect a block their local miners find. The only attested data is *shares* (+ uptime), which the lock will not honor except against an M-of-N signature.

---

## 4. Components

Each is a small, independently-testable unit.

1. **`pool-proxy`** (Node.js, extends `solo-proxy.js`) — accepts many miners; serves work from the operator's local CKB node (cellbase = pool lock); validates shares (Eaglesong recompute vs vardiff target); a network-difficulty share is also a block → submit to node + flag aggregator; buffers shares into **sequence-numbered, operator-signed batches** pushed to the aggregator.
   - *Deps:* local CKB node RPC/WS, operator signing key.
   - *Interface:* Stratum ↓ to miners; signed share-batch ↑ to aggregator.

2. **`aggregator`** (Node.js) — collects share-batches + uptime probes over the mesh; dedups/orders by `(operator, seq)`; maintains the PPLNS ledger; on block maturity computes the PPLNS allocation + uptime vector and builds the **unsigned** settlement tx; broadcasts `{unsigned_tx, raw signed batches, probe set}` to operators. **Not a custody or correctness authority** — liveness only.

3. **`operator-signer`** (Node.js) — on each operator; independently verifies batch signatures + sequence contiguity, **recomputes** PPLNS allocation and uptime vector, rebuilds the treasury transition, and co-signs **only if everything matches**. Holds that operator's M-of-N key.

4. **`liveness-prober`** (Node.js, co-located with proxy/aggregator) — periodically probes every operator endpoint (Stratum reachable **+** node serving a template whose cellbase pays the pool lock **+** mesh heartbeat); emits **signed** probe results into the mesh. Uptime per period = median across peer-probers (no single prober can over/under-report).

5. **`pool-payout-lock`** (Rust, on-chain) — the lock guarding the treasury + cellbase cells. Enforces authorization, fee cap + split, dust floor, accrual integrity, conservation, treasury well-formedness. Does **not** recompute PPLNS from raw shares.

6. **`treasury-cell` state machine** (TS lib) — pure functions modeling treasury cell data (`accrued_root`, `seq`, `params_hash`) + the legal settlement transition. **Mirror of the lock's logic**, sharing test vectors. Used by aggregator (build) and conceptually by the lock (verify).

7. **`dashboard`** (extends existing cyberpunk dashboard) — pool hashrate, per-operator uptime/contribution, per-miner accrued balance (read from on-chain root), settlement history.

8. **`mesh`** (Tailscale/Headscale config + thin RPC) — encrypted control-plane transport + operator identity.

**Deliberate mirrored logic:** #5 (Rust lock) *enforces* the same transition #6 (TS state machine) *constructs*. Shared test vectors across the two is the keystone correctness harness.

---

## 5. The money path

**Cellbase → treasury → settlement, all under `pool-payout-lock`.**

1. **Accumulate.** Each matured block's cellbase cell is locked by `pool-payout-lock`. Cellbase maturity (~4 epochs / ~16h) means settlement is **event-driven** (fires when a block matures), not clocked.

2. **Treasury cell** — singleton, same lock. `data = { accrued_root, seq, params_hash }`; `capacity = undistributed funds`.

3. **Settlement tx** (built by aggregator, co-signed ≥ M):
   - **Inputs:** treasury cell + newly-matured cellbase cells (+ small fee cell).
   - **Outputs:** recreated treasury cell (new `accrued_root`, leftover funds) · **trust output** = 50% of fee → governance trust address · **operator outputs** = 50% of fee weighted by signed **uptime vector** · **miner payout outputs** for every miner now ≥ 100 CKB.
   - **Witness:** ≥ M operator signatures over `(share_root, uptime_vector, tx)` + Merkle proofs for changed leaves.

### What the lock enforces (and deliberately does not)

The lock does **not** recompute PPLNS — too much data on-chain. It guarantees that **even M signatures cannot break the immutable rules**:

- **Authorization** — ≥ M valid sigs from the committed operator set.
- **Fee cap + split** — total fee ≤ **1%** of newly-settled reward; **exactly 50%** to the correct trust address; remainder to operator addresses matching the signed uptime weights.
- **Dust floor** — every miner output ≥ **100 CKB**; sub-floor miners remain in `accrued_root`.
- **Accrual integrity** — `new_root = old_root + credited − paid`, verified via supplied Merkle proofs.
- **Conservation** — `Σinputs = Σoutputs + tx_fee`; treasury recreated with same lock + unchanged `params_hash` (except via governance/rotation path).

### Trust boundary (stated plainly)

- **No unilateral theft, ever.** Any group `< M` can't move funds or break a rule. Found blocks are unstealable (PoW binds nonce to the pool-locked block).
- **Residual trust = "fewer than M operators collude."** M colluding signers could fabricate a share-root paying themselves as fake miners — the lock can't tell a real miner from an amount. **Verify-before-sign** defends this: honest operators won't sign a root that doesn't match the costly PoW shares they observed. Shares can't be fabricated without hashpower (only withheld, which only hurts the withholder's own miners). Security ⇒ standard **3-of-5 honest-majority** assumption.

---

## 6. Share accounting + verify-before-sign

### Share lifecycle
- **Miner identity = its CKB payout address** (`ckb1q…address.workername` as Stratum username, as the solo proxy already forwards). The address is the `accrued_root` leaf key. No account system.
- `pool-proxy` validates each share (Eaglesong recompute vs vardiff target), records `{ payout_addr, share_difficulty, timestamp, serving_operator, job_id }`. Network-difficulty share ⇒ block ⇒ submit + flag.
- Shares batched with a **per-operator monotonic sequence number**, operator-signed, pushed to aggregator. Sequence gives dedup + gap detection (a gap halts and asks).

### PPLNS scoring (difficulty-weighted)
- Scored by **cumulative share-difficulty**, not count (vardiff means a K7 share ≠ a NerdMiner share).
- On a block, the window is the run of shares back from the block whose cumulative difficulty sums to **W × D**. Each miner's slice of the 99% reward = their in-window difficulty ÷ total in-window difficulty.
- **W operator-configurable, default medium (~2×).** PPLNS chosen over PROP for hopping resistance; accepted trade-off is that shares older than the window earn nothing in long dry spells. PROP recorded as the simpler fallback.

### Settlement handshake
```
block matures (~16h)
  → aggregator: dedup+order batches → PPLNS allocation + uptime vector
               → build treasury transition + UNSIGNED settlement tx
               → broadcast {unsigned_tx, raw signed batches, probe set}
  → each operator-signer (independently):
        verify batch sigs + seq contiguity
        recompute PPLNS allocation     (must match)
        recompute uptime vector        (must match)
        rebuild treasury transition    (must match tx outputs)
        match → partial sign ; mismatch → refuse + alarm
  → aggregator: collect ≥ M sigs → assemble witness → broadcast
  → CKB L1: pool-payout-lock re-checks structure/caps/conservation → funds move
```
**Aggregator untrusted for correctness, trusted only for liveness.** A compromised aggregator can stall settlement (funds sit safely in pool-locked cells) but cannot misallocate a single CKB.

### Designed-for failure modes
- **Operator under-reports own shares** → hurts only that operator's own miners (visible on dashboard). Self-defeating.
- **Aggregator offline at maturity** → settlement waits; no funds at risk. (Rotating-leader upgrade removes the stall, if ever wanted.)

---

## 7. Economics reality (value prop honesty)

At community scale (~60 TH/s) vs a ~100 PH/s network, a solo block is ~5 months out; pooling ~10 K7s (~600 TH) is ~once every ~2 weeks. The pool's value is **pooled variance + localized low-latency getwork + a fair, auditable on-chain split** — not steady daily income. PPLNS + non-custodial requires no capital buffer, which is exactly why PPS was rejected.

---

## 8. Build phasing

- **Phase 1 — On-chain core (riskiest, first).** `pool-payout-lock` (Rust) + `treasury-cell` state machine (TS) with shared test vectors + settlement-tx builder. Deploy to testnet; exercise create → settlement → accrual with synthetic roots and a degenerate single signer (M=1). *Deliverable: fundable treasury cell, settle on testnet, all invariants verified — no mining yet.*
- **Phase 2 — Single-operator pool, end-to-end on testnet.** Extend `solo-proxy.js` → `pool-proxy` (multi-miner accounting, difficulty-weighted PPLNS, signed batches); aggregator + signer collapsed (1 operator, M=1). Point K7 at testnet (low difficulty ⇒ real blocks); full cellbase → treasury → settlement → miner payout with real PoW. *Deliverable: K7 mines testnet, miners paid via lock.*
- **Phase 3 — Federation.** Tailscale/Headscale mesh; 2–3 operators; `liveness-prober` → signed uptime vector; real **3-of-5** verify-before-sign; multi-operator aggregation/dedup; uptime fee split. *Deliverable: multi-operator testnet pool.*
- **Phase 4 — Harden + mainnet.** Lock **audit** (guards real value), dashboard (per-miner accrued from chain, operator uptime, settlement history), operator onboarding docs + `check-deps.sh`, tested rotation procedure, conservative mainnet launch.

---

## 9. Testing strategy

- **Keystone: shared test vectors between the Rust lock and the TS state machine** — same input ⇒ both agree on accept/reject *and* on resulting outputs.
- **Lock:** Rust unit tests + `ckb-debugger`, on-chain testnet integration, property tests on conservation + accrual, cycle benchmarking via `/ckb-bench`.
- **Adversarial verify-before-sign:** aggregator proposes bad txs (over-cap fee, wrong trust address, fabricated miner, broken accrual) → signer **must** refuse each. First-class tests.
- **`pool-proxy`:** Eaglesong share-validation vectors + K7 wire format (reuse proven proxy code), PPLNS scoring tests.
- 80% coverage target; mandatory security review on lock + signing paths.

---

## 10. Repo layout

New monorepo at `~/community-pool`, vendoring `ckb-stratum-proxy`:

```
community-pool/
  packages/
    pool-payout-lock/     # Rust on-chain lock (audited, versioned independently within repo)
    pool-core/            # TS: treasury-cell state machine, PPLNS, aggregator, operator-signer
    pool-proxy/           # extends ckb-stratum-proxy (vendored dep)
    liveness-prober/      # peer uptime probing
    dashboard/            # extends existing cyberpunk dashboard
  mesh/                   # Tailscale/Headscale config + onboarding
  docs/
    superpowers/specs/    # this spec
```

---

## 11. Open items for the implementation plan

- Concrete Merkle scheme for `accrued_root` (leaf = `hash(payout_addr || balance)`; sorted; proof format in witness).
- Exact `pool-payout-lock` args layout (operator pubkeys, trust address, fee-cap, threshold, W bounds, `params_hash`).
- Settlement-message serialization signed by operators (molecule schema).
- Governance/rotation tx shape (Phase 3+).
- CKB block-reward composition handling (primary + secondary + tx fees + proposal reward) when computing "newly-settled reward" for the fee base.
- Cellbase-cell consolidation strategy (sweep into treasury vs consume directly in settlement).
