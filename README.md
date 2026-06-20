# CKB Community Pool

A **federated, non-custodial PPLNS mining pool for CKB** (Eaglesong PoW). Community-owned, community-operated, and *trust-minimized by construction*: no operator — and no group of fewer than `M` operators — can custody, redirect, or steal pool funds. Found blocks are unstealable because the winning PoW nonce is cryptographically bound to a block that already pays the pool's on-chain lock.

> **Status:** Phase 1 in progress. The shared Sparse-Merkle-Tree accrual primitive (`pool-smt`) and its TypeScript wrapper (`pool-core`) are implemented and tested with Rust↔wasm byte-parity vectors. The on-chain payout lock, treasury state machine, proxy, aggregator, and federation mesh are designed and specced (see [`docs/superpowers/specs`](docs/superpowers/specs/)) and not yet implemented. This README describes the full target system and flags what exists today.

---

## Why this exists

Solo mining CKB at community hashrate (~60 TH/s against a ~100 PH/s network) means a block roughly every **4–5 hours on average — but with brutal Poisson variance**: you can easily go a full day with nothing, then catch two in an hour. Pooling smooths that bursty income into a steady cadence. Existing pool software (yiimp) is single-operator PHP/MySQL with no Eaglesong path and a custodial trust model. This project instead **extends the proven [`ckb-stratum-proxy`](https://github.com/toastmanAu)** (Eaglesong, K7/GodMiner wire format, vardiff, dashboard) and adds three things on top:

1. **Share accounting** — difficulty-weighted PPLNS over signed, sequence-numbered share batches.
2. **A custom on-chain payout lock** — funds move only against an `M`-of-`N` operator signature *and* a set of immutable rules the signatures cannot override.
3. **A federation mesh** — many independent operators run localized, low-latency getwork endpoints that link into one pool.

The value proposition is honest: **pooled variance + low-latency localized getwork + a fair, auditable on-chain split** — not steady daily income. Because payouts are PPLNS and non-custodial, the pool needs **no capital buffer**, which is exactly why PPS was rejected.

---

## Architecture

Two planes with different trust and latency requirements:

```
                          WORK PLANE (public TCP, low-latency getwork)
   miners ──stratum──▶ ┌──────────────┐   miners ──stratum──▶ ┌──────────────┐
   (nearest)           │  Operator A  │   (nearest)           │  Operator B  │
                       │  ckb node ───┤                       │  ckb node ───┤
                       │  pool-proxy  │                       │  pool-proxy  │
                       └──────┬───────┘                       └──────┬───────┘
   block_assembler = POOL LOCK  │  (same lock on every node)            │
 ─────────────────────────────┼───────────────────────────────────────┼──────
   CONTROL PLANE (Tailscale / Headscale — WireGuard, encrypted, NAT-traversed)
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
                                                    → miner / operator / trust payouts
```

**Foundational invariant:** every operator's CKB node sets `block_assembler` to the *same* pool lock. Any block found anywhere therefore pays the pool lock, and PoW binds the nonce to that exact block — so **the work plane is custody-trustless on its own.** A malicious operator cannot redirect a block their local miners find. The only attested off-chain data is *shares* (and uptime), which the lock will not honor except against an `M`-of-`N` signature.

---

## The money path

**Cellbase → treasury → settlement, all under one `pool-payout-lock`.**

1. **Accumulate.** Each matured block's cellbase cell is locked by `pool-payout-lock`. Cellbase maturity (~4 epochs / ~16h) makes settlement **event-driven** (fires when a block matures), not clocked.
2. **Treasury cell** — a singleton under the same lock. `data = { accrued_root, seq, params_hash }`; its `capacity` is the undistributed funds.
3. **Settlement tx** — built (unsigned) by the aggregator, co-signed by ≥ `M` operators:
   - **Inputs:** treasury cell + newly-matured cellbase cells (+ a small fee cell).
   - **Outputs:** recreated treasury (new `accrued_root`, leftover funds) · **trust output** = 50% of fee → governance treasury · **operator outputs** = 50% of fee weighted by the signed **uptime vector** · **miner payout outputs** for every miner now ≥ 100 CKB.
   - **Witness:** ≥ `M` operator signatures over `(share_root, uptime_vector, tx)` + Merkle proofs for the changed leaves.

### What the lock enforces (and deliberately does not)

The lock does **not** recompute PPLNS from raw shares — that's too much data on-chain. Instead it guarantees that **even `M` signatures cannot break the immutable rules**:

| Rule | Guarantee |
|------|-----------|
| **Authorization** | ≥ `M` valid sigs from the committed operator set. |
| **Fee cap + split** | Total fee ≤ **1%** of newly-settled reward; **exactly 50%** to the correct trust address; remainder to operator addresses matching the signed uptime weights. |
| **Dust floor** | Every miner output ≥ **100 CKB**; sub-floor miners stay accrued in the root (respects CKB's hard cell-capacity floor). |
| **Accrual integrity** | `new_root = old_root + credited − paid`, verified against supplied Merkle proofs. |
| **Conservation** | `Σinputs = Σoutputs + tx_fee`; treasury recreated under the same lock with unchanged `params_hash` (except via the governance/rotation path). |

### Trust boundary, stated plainly

- **No unilateral theft, ever.** Any group `< M` cannot move funds or break a rule.
- **Residual trust = "fewer than `M` operators collude."** `M` colluding signers could fabricate a share-root paying themselves as fake miners — the lock can't tell a real miner from an amount. **Verify-before-sign** defends this: honest operators recompute the allocation and refuse to sign a root that doesn't match the costly PoW shares they actually observed. Shares can't be fabricated without hashpower (only withheld, which only hurts the withholder's own miners). Security reduces to a standard **3-of-5 honest-majority** assumption.

---

## Share accounting + verify-before-sign

- **Miner identity = its CKB payout address** (`ckb1q…address.workername` as the Stratum username, as the solo proxy already forwards). The address is the `accrued_root` leaf key — no account system.
- `pool-proxy` validates each share (Eaglesong recompute vs the vardiff target) and records `{ payout_addr, share_difficulty, timestamp, serving_operator, job_id }`. A network-difficulty share *is* a block ⇒ submit to the node + flag the aggregator.
- Shares are batched with a **per-operator monotonic sequence number**, operator-signed, and pushed to the aggregator. The sequence gives dedup + gap detection (a gap halts and asks).

**PPLNS scoring** is difficulty-weighted (vardiff means a K7 share ≠ a NerdMiner share). On a block, the window is the run of shares back from the block whose cumulative difficulty sums to `W × D`; each miner's slice of the 99% reward = their in-window difficulty ÷ total in-window difficulty. **`W` is operator-configurable, default medium (~2×).** PPLNS was chosen over PROP for hopping resistance.

**Settlement handshake:**

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

The **aggregator is untrusted for correctness, trusted only for liveness.** A compromised or offline aggregator can *stall* settlement (funds sit safely in pool-locked cells) but cannot misallocate a single CKB.

---

## Components

| Component | Lang | Role |
|-----------|------|------|
| `pool-smt` | Rust → wasm | **Implemented.** Sparse-Merkle-Tree accrual primitive: `key→value` root + Merkle proof gen/verify. Compiled to wasm for off-chain use and (later) to RISC-V for on-chain verification — byte-identical roots/proofs. |
| `pool-core` | TS | **Partial.** Wraps `pool-smt` wasm (`accrualRoot`, `genProof`, `verifyProof`); will host the treasury-cell state machine, PPLNS, aggregator, and operator-signer. |
| `pool-payout-lock` | Rust (on-chain) | *Designed.* The lock guarding treasury + cellbase cells. Enforces authorization, fee cap+split, dust floor, accrual integrity, conservation. |
| `treasury-cell` state machine | TS | *Designed.* Pure functions modeling treasury cell data + the legal settlement transition. **Mirror of the lock's logic, sharing test vectors.** |
| `pool-proxy` | Node.js | *Designed.* Extends `ckb-stratum-proxy`: multi-miner accounting, Eaglesong share validation, signed share-batches. |
| `aggregator` | Node.js | *Designed.* PPLNS ledger + unsigned settlement tx builder. Liveness-only authority. |
| `operator-signer` | Node.js | *Designed.* Independently recomputes and co-signs only on a full match. |
| `liveness-prober` | Node.js | *Designed.* Peer uptime probing → signed uptime vector (median across probers). |
| `dashboard` | TS | *Designed.* Pool hashrate, per-operator uptime, per-miner accrued balance (from chain), settlement history. |
| `mesh` | Headscale/Tailscale | *Designed.* Encrypted WireGuard control plane + operator identity. |

**Deliberate mirrored logic** is the keystone: the Rust lock *enforces* the same transition the TS state machine *constructs*, and shared test vectors across the two are the primary correctness harness.

---

## The accrual SMT (`pool-smt`) — implemented today

The one piece fully built so far. It commits per-miner accrued balances as a single 32-byte root with verifiable Merkle proofs.

- **Implementation:** one Rust SMT (vendored `sparse-merkle-tree v0.6.1` + CKB-personalized blake2b), compiled to wasm via `wasm-bindgen` / `wasm-pack`. The *same* crate will compile to RISC-V on-chain, so roots and proofs are byte-identical off- and on-chain.
- **Keys** = a miner's payout-lock hash (`Byte32`, `0x` + 64 hex). In production: `blake2b("ckb-default-hash", molecule(lock_script))` — i.e. CCC's `script.hash()`.
- **Values** = accrued balance in **shannons** (`u64`), little-endian in bytes `[0..8]` of a 32-byte value. Balance `0` ⇒ all-zero value ⇒ the SMT prunes the key, so a fully-paid-out miner drops out of the tree cleanly.
- **Boundary:** balances cross the wasm boundary as **decimal strings**, never JS `Number`, to avoid precision loss above 2⁵³.
- **Empty-tree root** = `0x` + 64 zeros. All hex lowercase, `0x`-prefixed.
- **Shared vectors:** `packages/pool-core/test/vectors/smt-vectors.json` is the canonical contract — TS tests assert against it, native Rust tests assert against it, and a wasm-generated set is asserted byte-equal to the native one. This is what the future on-chain lock will replay to prove parity.

---

## Repo layout

```
community-pool/
  packages/
    pool-smt/              # Rust SMT crate → wasm (implemented)
      src/
        lib.rs             #   wasm-bindgen surface (thin)
        balance.rs         #   key/value encoding (the only place it lives)
        smt.rs             #   pure root / gen_proof / verify_proof
      vendor/
        sparse-merkle-tree #   vendored v0.6.1 + CKB blake2b
      tests/vectors.rs     #   native test over the shared vectors
      pkg/                 #   wasm-pack output (committed for the TS dep)
    pool-core/             # TS wrapper + (future) state machine, PPLNS, aggregator
      src/smt.ts
      test/                #   round-trip + tamper tests, shared vectors
      scripts/gen-vectors.ts
  docs/
    superpowers/specs/     # full design spec
    superpowers/plans/     # implementation plans
```

---

## Build & test

**Prerequisites:** Rust (cargo ≥ 1.92), `wasm-pack` ≥ 0.13, Node ≥ 22.

```bash
# Rust SMT — native unit + vector tests
cd packages/pool-smt
cargo test

# (Re)build the wasm package consumed by pool-core
wasm-pack build --target web --out-dir pkg

# TypeScript wrapper — round-trip, tamper, and parity tests
cd ../pool-core
npm install
npm test

# Regenerate the canonical shared vectors from the wasm (only when the
# encoding intentionally changes — this file is a cross-language contract)
npm run gen-vectors
```

---

## Build phasing

- **Phase 1 — On-chain core (riskiest, first).** ✅ accrual SMT + shared vectors done. → `pool-payout-lock` (Rust) + `treasury-cell` state machine (TS) with shared vectors + settlement-tx builder; deploy to testnet; exercise create → settlement → accrual with a degenerate single signer (`M=1`).
- **Phase 2 — Single-operator pool, end-to-end on testnet.** `solo-proxy.js` → `pool-proxy` (multi-miner accounting, difficulty-weighted PPLNS, signed batches); aggregator + signer collapsed. Point a K7 at testnet (low difficulty ⇒ real blocks); full cellbase → treasury → settlement → miner payout with real PoW.
- **Phase 3 — Federation.** Tailscale/Headscale mesh; 2–3 operators; `liveness-prober` → signed uptime vector; real **3-of-5** verify-before-sign; uptime fee split.
- **Phase 4 — Harden + mainnet.** Lock **audit** (guards real value), dashboard, operator onboarding + `check-deps.sh`, tested rotation procedure, conservative mainnet launch.

---

## Security model in one paragraph

Funds live in cells under `pool-payout-lock`. PoW binds every found block to the pool lock, so no operator can redirect a block. Funds move only via a settlement tx carrying ≥ `M`-of-`N` operator signatures, and the lock independently re-checks authorization, the 1% fee cap, the 50/50 split, the 100-CKB dust floor, accrual integrity (`new = old + credited − paid` via Merkle proofs), and conservation. Signatures cannot override these rules. The only residual trust is collusion of ≥ `M` operators, which **verify-before-sign** neutralizes in practice because honest operators recompute every allocation from the PoW shares they observed and refuse to sign anything that doesn't match. The lock and the signing paths get a mandatory security review and an external audit before mainnet.

---

## License

MIT (see crate manifests). On-chain code handles real value — treat it as financial software and review accordingly.
