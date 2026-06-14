# Pool SMT Primitive (accrual root + proofs) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build the shared Sparse-Merkle-Tree primitive (`pool-smt`) that commits per-miner accrued balances as a `key→value` root with verifiable Merkle proofs, exposed to TypeScript via WebAssembly, plus a canonical test-vector set that the on-chain Rust lock will later consume to prove byte-parity.

**Architecture:** One Rust SMT implementation (vendored `sparse-merkle-tree v0.6.1` + pure-Rust CKB-personalized blake2b), compiled to wasm for the off-chain TS state machine. Keys are miner payout-lock hashes (`Byte32`); values are accrued balances in shannons (`u64`, encoded little-endian into a 32-byte value). The same crate, compiled to RISC-V with the `smtc` feature, will verify proofs on-chain in a later plan — roots and proofs are byte-identical because both use the identical algorithm + hasher. This plan delivers the off-chain half plus the shared vectors.

**Tech Stack:** Rust (cargo 1.92), `sparse-merkle-tree v0.6.1` (vendored), `blake2b_simd`, `wasm-bindgen` + `wasm-pack 0.13.1` (target web), TypeScript + Vitest 2.x, Node 22.

This is **Phase 1, Part A** of the CKB Community Pool (see `docs/superpowers/specs/2026-06-14-ckb-community-pool-design.md`). It unblocks the Rust `pool-payout-lock` plan (Part B) and the `treasury-cell` state machine (Part C), both of which depend on this root/proof primitive.

---

## File Structure

```
community-pool/
  packages/
    pool-smt/                      # Rust crate → wasm
      Cargo.toml
      src/
        lib.rs                     # wasm-bindgen exports (thin)
        balance.rs                 # BalanceValue + key/value encoding (pure, testable)
        smt.rs                     # root + proof gen + verify inner fns (pure, testable)
      vendor/
        sparse-merkle-tree/        # copied from ~/ckb-smt-wasm/vendor
      tests/
        vectors.rs                 # native test: load shared vectors, assert root+proof
    pool-core/                     # TS
      package.json
      vitest.config.ts
      src/
        smt.ts                     # TS wrapper around the wasm pkg
      test/
        smt.test.ts                # round-trip + tamper tests
        vectors/
          smt-vectors.json         # canonical shared vectors (committed)
      scripts/
        gen-vectors.ts             # regenerates smt-vectors.json from the wasm
```

**Responsibilities:**
- `balance.rs` — the *only* place the addr→key and balance→value encoding lives. Mirrored on-chain later; isolating it makes parity auditable.
- `smt.rs` — pure functions: `root`, `gen_proof`, `verify`. No wasm types, so native `cargo test` exercises them directly.
- `lib.rs` — wasm-bindgen surface only; delegates to `smt.rs`. Keeps the FFI boundary trivial.
- `smt.ts` — loads the wasm once, exposes `accrualRoot`, `genProof`, `verifyProof` to the rest of `pool-core`.
- `smt-vectors.json` — the contract between this plan, the TS tests, and the future Rust-lock plan.

---

## Conventions (read once before starting)

- **Key** = a miner's payout-lock hash: a 32-byte value, hex string `0x` + 64 chars. In tests we use synthetic hashes; in production it's `blake2b("ckb-default-hash", molecule(lock_script))`, which CCC computes as `script.hash()`.
- **Balance** = accrued amount in **shannons** (`1 CKB = 100_000_000 shannons`), a `u64`. Passed across the wasm boundary as a **decimal string** to avoid JS `Number` precision loss above 2^53.
- **Value encoding** = balance `u64` little-endian in bytes `[0..8]` of a 32-byte array, bytes `[8..32]` zero. Balance `0` ⇒ all-zero value ⇒ the SMT treats the key as absent (matches the crate's zero-value pruning). This is why a miner whose accrued balance is fully paid out drops out of the tree cleanly.
- **Empty tree root** = `0x` + 64 zeros.
- All hex is lowercase, `0x`-prefixed.

---

## Task 0: Scaffold the `pool-smt` crate

**Files:**
- Create: `packages/pool-smt/Cargo.toml`
- Create: `packages/pool-smt/src/lib.rs`
- Copy: `packages/pool-smt/vendor/sparse-merkle-tree/` (from `~/ckb-smt-wasm/vendor/sparse-merkle-tree`)

- [ ] **Step 1: Copy the vendored SMT crate**

```bash
mkdir -p ~/community-pool/packages/pool-smt/src
cp -r ~/ckb-smt-wasm/vendor ~/community-pool/packages/pool-smt/vendor
ls ~/community-pool/packages/pool-smt/vendor/sparse-merkle-tree/Cargo.toml
```
Expected: the path prints (file exists).

- [ ] **Step 2: Write `Cargo.toml`**

Create `packages/pool-smt/Cargo.toml`:

```toml
[package]
name = "pool-smt"
version = "0.1.0"
edition = "2021"
description = "CKB-compatible Sparse Merkle Tree for community-pool accrual (balance key->value root + proofs, WASM)"
license = "MIT"

[lib]
crate-type = ["cdylib", "rlib"]

[dependencies]
sparse-merkle-tree = { path = "vendor/sparse-merkle-tree", default-features = false, features = ["std"] }
blake2b_simd = "1"
wasm-bindgen = "0.2"
hex = "0.4"

[dev-dependencies]
serde = { version = "1", features = ["derive"] }
serde_json = "1"

[profile.release]
opt-level = "z"
lto = true
codegen-units = 1
```

- [ ] **Step 3: Write a placeholder `src/lib.rs` so it compiles**

Create `packages/pool-smt/src/lib.rs`:

```rust
//! Community-pool accrual SMT: balance key->value root + Merkle proofs.
//! Pure logic lives in `balance` and `smt`; this file is the wasm surface.

mod balance;
mod smt;
```

Create empty modules so it builds. Create `packages/pool-smt/src/balance.rs`:

```rust
//! Encoding of a miner payout-lock hash (key) and accrued balance (value).
```

Create `packages/pool-smt/src/smt.rs`:

```rust
//! Pure SMT operations: root, proof generation, proof verification.
```

- [ ] **Step 4: Verify it builds**

Run: `cd ~/community-pool/packages/pool-smt && cargo build`
Expected: compiles with warnings about unused modules; no errors.

- [ ] **Step 5: Commit**

```bash
cd ~/community-pool
git add packages/pool-smt
git commit -m "chore: scaffold pool-smt crate with vendored sparse-merkle-tree"
```

---

## Task 1: `BalanceValue` + key/value encoding

**Files:**
- Modify: `packages/pool-smt/src/balance.rs`
- Test: native `#[cfg(test)]` in `balance.rs`

- [ ] **Step 1: Write the failing test**

Append to `packages/pool-smt/src/balance.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use sparse_merkle_tree::traits::Value;

    #[test]
    fn balance_zero_encodes_to_zero_h256() {
        let v = BalanceValue(0);
        assert!(v.to_h256().is_zero());
    }

    #[test]
    fn balance_one_ckb_encodes_le_in_low_8_bytes() {
        let v = BalanceValue(100_000_000); // 1 CKB in shannons
        let h = v.to_h256();
        let bytes = h.as_slice();
        // 100_000_000 = 0x05F5E100 -> LE bytes 00 E1 F5 05 00 00 00 00
        assert_eq!(&bytes[0..8], &100_000_000u64.to_le_bytes());
        assert_eq!(&bytes[8..32], &[0u8; 24]);
    }

    #[test]
    fn parse_key_round_trips() {
        let hex = format!("0x{}", "ab".repeat(32));
        let key = parse_key(&hex).unwrap();
        assert_eq!(key.as_slice(), &[0xabu8; 32]);
    }

    #[test]
    fn parse_key_rejects_wrong_length() {
        assert!(parse_key("0x1234").is_err());
    }
}
```

- [ ] **Step 2: Run to verify it fails**

Run: `cd ~/community-pool/packages/pool-smt && cargo test balance`
Expected: FAIL — `BalanceValue`, `parse_key` not found.

- [ ] **Step 3: Implement**

Replace the contents of `packages/pool-smt/src/balance.rs` ABOVE the `#[cfg(test)]` block with:

```rust
//! Encoding of a miner payout-lock hash (key) and accrued balance (value).

use sparse_merkle_tree::{traits::Value, H256};

/// Accrued balance in shannons (1 CKB = 100_000_000 shannons).
/// Encoded little-endian into the low 8 bytes of the 32-byte SMT value.
/// Balance 0 -> all-zero value -> key absent from the tree (crate prunes it).
#[derive(Clone, Copy, Default, Debug, PartialEq, Eq)]
pub struct BalanceValue(pub u64);

impl Value for BalanceValue {
    fn to_h256(&self) -> H256 {
        let mut v = [0u8; 32];
        v[0..8].copy_from_slice(&self.0.to_le_bytes());
        v.into()
    }
    fn zero() -> Self {
        BalanceValue(0)
    }
}

/// Parse a `0x`-prefixed 32-byte hex string into an SMT key.
pub fn parse_key(input: &str) -> Result<H256, String> {
    let trimmed = input.trim_start_matches("0x");
    let bytes = hex::decode(trimmed).map_err(|e| format!("hex decode failed: {e}"))?;
    if bytes.len() != 32 {
        return Err(format!("expected 32-byte key, got {}", bytes.len()));
    }
    let mut arr = [0u8; 32];
    arr.copy_from_slice(&bytes);
    Ok(H256::from(arr))
}

/// Parse a decimal shannon string into a `BalanceValue`.
/// Decimal string (not JS number) preserves u64 precision across the wasm boundary.
pub fn parse_balance(input: &str) -> Result<BalanceValue, String> {
    input
        .parse::<u64>()
        .map(BalanceValue)
        .map_err(|e| format!("balance parse failed: {e}"))
}
```

- [ ] **Step 4: Run to verify it passes**

Run: `cd ~/community-pool/packages/pool-smt && cargo test balance`
Expected: PASS (4 tests).

- [ ] **Step 5: Commit**

```bash
cd ~/community-pool
git add packages/pool-smt/src/balance.rs
git commit -m "feat(pool-smt): BalanceValue + key/balance parsing"
```

---

## Task 2: Root, proof generation, and verification

**Files:**
- Modify: `packages/pool-smt/src/smt.rs`
- Test: native `#[cfg(test)]` in `smt.rs`

- [ ] **Step 1: Write the failing test**

Append to `packages/pool-smt/src/smt.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    fn k(byte: u8) -> String {
        format!("0x{}", format!("{:02x}", byte).repeat(32))
    }

    #[test]
    fn empty_root_is_zero() {
        let root = root_inner(&[], &[]).unwrap();
        assert_eq!(root, format!("0x{}", "0".repeat(64)));
    }

    #[test]
    fn root_is_order_independent() {
        let a = root_inner(&[k(1), k(2)], &["100".into(), "200".into()]).unwrap();
        let b = root_inner(&[k(2), k(1)], &["200".into(), "100".into()]).unwrap();
        assert_eq!(a, b);
    }

    #[test]
    fn proof_verifies_for_included_key() {
        let keys = vec![k(1), k(2), k(3)];
        let bals = vec!["100".to_string(), "200".to_string(), "300".to_string()];
        let root = root_inner(&keys, &bals).unwrap();
        let proof = gen_proof_inner(&keys, &bals, &[k(2)]).unwrap();
        let ok = verify_inner(&root, &proof, &[k(2)], &["200".into()]).unwrap();
        assert!(ok);
    }

    #[test]
    fn proof_fails_for_wrong_balance() {
        let keys = vec![k(1), k(2)];
        let bals = vec!["100".to_string(), "200".to_string()];
        let root = root_inner(&keys, &bals).unwrap();
        let proof = gen_proof_inner(&keys, &bals, &[k(2)]).unwrap();
        let ok = verify_inner(&root, &proof, &[k(2)], &["999".into()]).unwrap();
        assert!(!ok);
    }

    #[test]
    fn proof_verifies_absence() {
        let keys = vec![k(1)];
        let bals = vec!["100".to_string()];
        let root = root_inner(&keys, &bals).unwrap();
        // k(9) absent -> balance 0
        let proof = gen_proof_inner(&keys, &bals, &[k(9)]).unwrap();
        let ok = verify_inner(&root, &proof, &[k(9)], &["0".into()]).unwrap();
        assert!(ok);
    }
}
```

- [ ] **Step 2: Run to verify it fails**

Run: `cd ~/community-pool/packages/pool-smt && cargo test smt`
Expected: FAIL — `root_inner`, `gen_proof_inner`, `verify_inner` not found.

- [ ] **Step 3: Implement**

Replace the contents of `packages/pool-smt/src/smt.rs` ABOVE the `#[cfg(test)]` block with:

```rust
//! Pure SMT operations: root, proof generation, proof verification.
//! All inner fns take/return strings so native tests and the wasm surface
//! share one code path.

use blake2b_simd::{Params, State};
use sparse_merkle_tree::{
    default_store::DefaultStore, traits::Hasher, traits::Value, MerkleProof, SparseMerkleTree, H256,
};

use crate::balance::{parse_balance, parse_key, BalanceValue};

/// CKB-personalized blake2b (32-byte digest, personalization "ckb-default-hash").
/// blake2b_simd produces byte-identical output to the on-chain C hasher, which is
/// what makes off-chain (wasm) and on-chain (RISC-V) roots/proofs match.
pub struct Blake2bHasher(State);

fn ckb_blake2b_params() -> Params {
    let mut p = Params::new();
    p.hash_length(32).personal(b"ckb-default-hash");
    p
}

impl Default for Blake2bHasher {
    fn default() -> Self {
        Blake2bHasher(ckb_blake2b_params().to_state())
    }
}

impl Hasher for Blake2bHasher {
    fn write_h256(&mut self, h: &H256) {
        self.0.update(h.as_slice());
    }
    fn write_byte(&mut self, b: u8) {
        self.0.update(&[b]);
    }
    fn finish(self) -> H256 {
        let mut hash = [0u8; 32];
        hash.copy_from_slice(self.0.finalize().as_bytes());
        hash.into()
    }
}

type Smt = SparseMerkleTree<Blake2bHasher, BalanceValue, DefaultStore<BalanceValue>>;

fn build_tree(keys: &[String], balances: &[String]) -> Result<Smt, String> {
    if keys.len() != balances.len() {
        return Err(format!(
            "keys/balances length mismatch: {} vs {}",
            keys.len(),
            balances.len()
        ));
    }
    let mut smt = Smt::default();
    for (k, b) in keys.iter().zip(balances.iter()) {
        let key = parse_key(k)?;
        let val = parse_balance(b)?;
        smt.update(key, val).map_err(|e| format!("smt update failed: {e}"))?;
    }
    Ok(smt)
}

/// Compute the accrual root over (keys, balances). Empty -> 32 zero bytes.
pub fn root_inner(keys: &[String], balances: &[String]) -> Result<String, String> {
    if keys.is_empty() {
        return Ok(format!("0x{}", "0".repeat(64)));
    }
    let smt = build_tree(keys, balances)?;
    Ok(format!("0x{}", hex::encode(smt.root().as_slice())))
}

/// Generate a compiled Merkle proof for `proof_keys` against the tree built
/// from (keys, balances). Returns the compiled proof as a `0x`-hex string.
pub fn gen_proof_inner(
    keys: &[String],
    balances: &[String],
    proof_keys: &[String],
) -> Result<String, String> {
    let smt = build_tree(keys, balances)?;
    let pk: Result<Vec<H256>, String> = proof_keys.iter().map(|k| parse_key(k)).collect();
    let pk = pk?;
    let proof: MerkleProof = smt
        .merkle_proof(pk.clone())
        .map_err(|e| format!("merkle_proof failed: {e}"))?;
    let compiled = proof
        .compile(pk)
        .map_err(|e| format!("compile failed: {e}"))?;
    Ok(format!("0x{}", hex::encode(compiled.0)))
}

/// Verify a compiled proof: does `(proof_keys, proof_balances)` belong to `root`?
pub fn verify_inner(
    root: &str,
    compiled_proof: &str,
    proof_keys: &[String],
    proof_balances: &[String],
) -> Result<bool, String> {
    if proof_keys.len() != proof_balances.len() {
        return Err("proof keys/balances length mismatch".to_string());
    }
    let root_h = parse_key(root)?;
    let proof_bytes = hex::decode(compiled_proof.trim_start_matches("0x"))
        .map_err(|e| format!("proof hex decode failed: {e}"))?;
    let leaves: Result<Vec<(H256, H256)>, String> = proof_keys
        .iter()
        .zip(proof_balances.iter())
        .map(|(k, b)| Ok((parse_key(k)?, parse_balance(b)?.to_h256())))
        .collect();
    let leaves = leaves?;
    let compiled = sparse_merkle_tree::CompiledMerkleProof(proof_bytes);
    compiled
        .verify::<Blake2bHasher>(&root_h, leaves)
        .map_err(|e| format!("verify failed: {e}"))
}
```

> Note: `parse_key` is reused to parse the 32-byte root hex (same shape). If the vendored crate's `MerkleProof::compile` signature differs, check `vendor/sparse-merkle-tree/src/merkle_proof.rs` — v0.6.1 exposes `compile(self, leaves_keys: Vec<H256>) -> Result<CompiledMerkleProof>` and `CompiledMerkleProof(pub Vec<u8>)` with `verify<H: Hasher + Default>(&self, root: &H256, leaves: Vec<(H256, H256)>) -> Result<bool>`.

- [ ] **Step 4: Run to verify it passes**

Run: `cd ~/community-pool/packages/pool-smt && cargo test smt`
Expected: PASS (5 tests). If `compile`/`verify` signatures mismatch, reconcile against the vendored source as noted, then re-run.

- [ ] **Step 5: Commit**

```bash
cd ~/community-pool
git add packages/pool-smt/src/smt.rs
git commit -m "feat(pool-smt): accrual root + merkle proof gen/verify"
```

---

## Task 3: wasm-bindgen surface + build the wasm package

**Files:**
- Modify: `packages/pool-smt/src/lib.rs`

- [ ] **Step 1: Implement the wasm surface**

Replace `packages/pool-smt/src/lib.rs` with:

```rust
//! Community-pool accrual SMT: balance key->value root + Merkle proofs.
//! Pure logic lives in `balance` and `smt`; this file is the wasm surface.

mod balance;
mod smt;

use wasm_bindgen::prelude::*;

/// Accrual root over parallel (keys, balances). Balances are decimal shannon strings.
#[wasm_bindgen]
pub fn accrual_root(keys: Vec<String>, balances: Vec<String>) -> Result<String, JsError> {
    smt::root_inner(&keys, &balances).map_err(|e| JsError::new(&e))
}

/// Compiled Merkle proof (0x-hex) for `proof_keys` against the (keys, balances) tree.
#[wasm_bindgen]
pub fn gen_proof(
    keys: Vec<String>,
    balances: Vec<String>,
    proof_keys: Vec<String>,
) -> Result<String, JsError> {
    smt::gen_proof_inner(&keys, &balances, &proof_keys).map_err(|e| JsError::new(&e))
}

/// Verify a compiled proof against a root.
#[wasm_bindgen]
pub fn verify_proof(
    root: String,
    compiled_proof: String,
    proof_keys: Vec<String>,
    proof_balances: Vec<String>,
) -> Result<bool, JsError> {
    smt::verify_inner(&root, &compiled_proof, &proof_keys, &proof_balances)
        .map_err(|e| JsError::new(&e))
}
```

- [ ] **Step 2: Make `balance`/`smt` items visible to `lib.rs`**

The inner fns are already `pub`. Confirm `cargo build` still passes:

Run: `cd ~/community-pool/packages/pool-smt && cargo build`
Expected: compiles, no errors.

- [ ] **Step 3: Build the wasm package**

Run:
```bash
cd ~/community-pool/packages/pool-smt && wasm-pack build --target web --out-dir pkg
ls pkg
```
Expected: `pkg/` contains `pool_smt.js`, `pool_smt_bg.wasm`, `pool_smt.d.ts`, `package.json`. The `.d.ts` declares `accrual_root`, `gen_proof`, `verify_proof`.

- [ ] **Step 4: Commit (ignore build artifacts in git, keep pkg buildable)**

Add `packages/pool-smt/.gitignore`:
```
/target
/pkg
```

```bash
cd ~/community-pool
git add packages/pool-smt/src/lib.rs packages/pool-smt/.gitignore
git commit -m "feat(pool-smt): wasm-bindgen exports (accrual_root, gen_proof, verify_proof)"
```

---

## Task 4: TS wrapper in `pool-core` + round-trip tests

**Files:**
- Create: `packages/pool-core/package.json`
- Create: `packages/pool-core/vitest.config.ts`
- Create: `packages/pool-core/src/smt.ts`
- Test: `packages/pool-core/test/smt.test.ts`

- [ ] **Step 1: Scaffold the TS package**

Create `packages/pool-core/package.json`:
```json
{
  "name": "@community-pool/core",
  "version": "0.1.0",
  "type": "module",
  "private": true,
  "scripts": {
    "test": "vitest run",
    "test:watch": "vitest",
    "gen-vectors": "tsx scripts/gen-vectors.ts"
  },
  "dependencies": {
    "pool-smt": "file:../pool-smt/pkg"
  },
  "devDependencies": {
    "vitest": "^2.1.0",
    "tsx": "^4.19.0",
    "typescript": "^5.6.0"
  }
}
```

Create `packages/pool-core/vitest.config.ts`:
```ts
import { defineConfig } from 'vitest/config'

export default defineConfig({
  test: {
    environment: 'node',
    include: ['test/**/*.test.ts'],
  },
})
```

- [ ] **Step 2: Install (wasm pkg must be built first — Task 3)**

Run:
```bash
cd ~/community-pool/packages/pool-core && npm install
```
Expected: installs vitest, tsx, and the local `pool-smt` file dependency. No errors.

- [ ] **Step 3: Write the failing test**

Create `packages/pool-core/test/smt.test.ts`:
```ts
import { describe, it, expect, beforeAll } from 'vitest'
import { initSmt, accrualRoot, genProof, verifyProof } from '../src/smt.js'

const k = (b: number) =>
  '0x' + b.toString(16).padStart(2, '0').repeat(32)

beforeAll(async () => {
  await initSmt()
})

describe('pool-smt TS wrapper', () => {
  it('empty tree root is all zeros', () => {
    expect(accrualRoot([])).toBe('0x' + '0'.repeat(64))
  })

  it('root is order-independent', () => {
    const a = accrualRoot([
      { lockHash: k(1), shannons: 100n },
      { lockHash: k(2), shannons: 200n },
    ])
    const b = accrualRoot([
      { lockHash: k(2), shannons: 200n },
      { lockHash: k(1), shannons: 100n },
    ])
    expect(a).toBe(b)
  })

  it('proof round-trips for an included key', () => {
    const entries = [
      { lockHash: k(1), shannons: 100n },
      { lockHash: k(2), shannons: 200n },
      { lockHash: k(3), shannons: 300n },
    ]
    const root = accrualRoot(entries)
    const proof = genProof(entries, [k(2)])
    expect(verifyProof(root, proof, [{ lockHash: k(2), shannons: 200n }])).toBe(true)
  })

  it('proof fails for a tampered balance', () => {
    const entries = [
      { lockHash: k(1), shannons: 100n },
      { lockHash: k(2), shannons: 200n },
    ]
    const root = accrualRoot(entries)
    const proof = genProof(entries, [k(2)])
    expect(verifyProof(root, proof, [{ lockHash: k(2), shannons: 999n }])).toBe(false)
  })
})
```

- [ ] **Step 4: Run to verify it fails**

Run: `cd ~/community-pool/packages/pool-core && npm test`
Expected: FAIL — `../src/smt.js` not found.

- [ ] **Step 5: Implement the wrapper**

Create `packages/pool-core/src/smt.ts`:
```ts
import init, {
  accrual_root,
  gen_proof,
  verify_proof,
} from 'pool-smt'

export interface AccrualEntry {
  /** Miner payout-lock hash: 0x + 64 hex chars. */
  lockHash: string
  /** Accrued balance in shannons. */
  shannons: bigint
}

let ready = false

/** Load the wasm module once. Call before any other export. */
export async function initSmt(): Promise<void> {
  if (!ready) {
    await init()
    ready = true
  }
}

function split(entries: AccrualEntry[]): { keys: string[]; balances: string[] } {
  return {
    keys: entries.map((e) => e.lockHash),
    balances: entries.map((e) => e.shannons.toString()),
  }
}

/** Accrual root over the entries (order-independent). */
export function accrualRoot(entries: AccrualEntry[]): string {
  const { keys, balances } = split(entries)
  return accrual_root(keys, balances)
}

/** Compiled Merkle proof (0x-hex) for the given keys against the entries' tree. */
export function genProof(entries: AccrualEntry[], proofKeys: string[]): string {
  const { keys, balances } = split(entries)
  return gen_proof(keys, balances, proofKeys)
}

/** Verify a compiled proof against a root for the claimed (key, balance) leaves. */
export function verifyProof(
  root: string,
  compiledProof: string,
  leaves: AccrualEntry[],
): boolean {
  const keys = leaves.map((l) => l.lockHash)
  const balances = leaves.map((l) => l.shannons.toString())
  return verify_proof(root, compiledProof, keys, balances)
}
```

- [ ] **Step 6: Run to verify it passes**

Run: `cd ~/community-pool/packages/pool-core && npm test`
Expected: PASS (4 tests). If the wasm `init()` default export path differs, check `pkg/pool_smt.d.ts` for the exact init signature and adjust the import.

- [ ] **Step 7: Commit**

Add `packages/pool-core/.gitignore`:
```
node_modules
```

```bash
cd ~/community-pool
git add packages/pool-core/package.json packages/pool-core/package-lock.json \
  packages/pool-core/vitest.config.ts packages/pool-core/src/smt.ts \
  packages/pool-core/test/smt.test.ts packages/pool-core/.gitignore
git commit -m "feat(pool-core): TS SMT wrapper + round-trip tests over wasm"
```

---

## Task 5: Canonical shared vectors

**Files:**
- Create: `packages/pool-core/scripts/gen-vectors.ts`
- Create: `packages/pool-core/test/vectors/smt-vectors.json` (generated, committed)
- Test: extend `packages/pool-core/test/smt.test.ts`

- [ ] **Step 1: Write the vector generator**

Create `packages/pool-core/scripts/gen-vectors.ts`:
```ts
import { writeFileSync, mkdirSync } from 'node:fs'
import { dirname } from 'node:path'
import { initSmt, accrualRoot, genProof, type AccrualEntry } from '../src/smt.js'

const k = (b: number) => '0x' + b.toString(16).padStart(2, '0').repeat(32)

interface Vector {
  name: string
  entries: { lockHash: string; shannons: string }[]
  root: string
  proofKey: string
  compiledProof: string
  proofBalance: string // expected balance for proofKey ("0" if absent)
}

async function main() {
  await initSmt()

  const cases: { name: string; entries: AccrualEntry[]; proofKey: string }[] = [
    {
      name: 'three-miners',
      entries: [
        { lockHash: k(1), shannons: 100_000_000n },
        { lockHash: k(2), shannons: 250_000_000n },
        { lockHash: k(3), shannons: 61_000_000_00n },
      ],
      proofKey: k(2),
    },
    {
      name: 'absence',
      entries: [{ lockHash: k(1), shannons: 100_000_000n }],
      proofKey: k(9),
    },
  ]

  const vectors: Vector[] = cases.map((c) => {
    const root = accrualRoot(c.entries)
    const compiledProof = genProof(c.entries, [c.proofKey])
    const match = c.entries.find((e) => e.lockHash === c.proofKey)
    return {
      name: c.name,
      entries: c.entries.map((e) => ({ lockHash: e.lockHash, shannons: e.shannons.toString() })),
      root,
      proofKey: c.proofKey,
      compiledProof,
      proofBalance: (match?.shannons ?? 0n).toString(),
    }
  })

  const out = 'test/vectors/smt-vectors.json'
  mkdirSync(dirname(out), { recursive: true })
  writeFileSync(out, JSON.stringify(vectors, null, 2) + '\n')
  console.log(`wrote ${vectors.length} vectors to ${out}`)
}

main()
```

- [ ] **Step 2: Generate the vectors**

Run:
```bash
cd ~/community-pool/packages/pool-core && npm run gen-vectors
cat test/vectors/smt-vectors.json | head -20
```
Expected: prints "wrote 2 vectors"; the JSON shows `root` and `compiledProof` as `0x…` hex.

- [ ] **Step 3: Write the failing test that consumes the vectors**

Append to `packages/pool-core/test/smt.test.ts`:
```ts
import vectors from './vectors/smt-vectors.json' assert { type: 'json' }

describe('canonical vectors', () => {
  for (const v of vectors as any[]) {
    it(`root matches for "${v.name}"`, () => {
      const entries = v.entries.map((e: any) => ({
        lockHash: e.lockHash,
        shannons: BigInt(e.shannons),
      }))
      expect(accrualRoot(entries)).toBe(v.root)
    })

    it(`proof verifies for "${v.name}"`, () => {
      const ok = verifyProof(v.root, v.compiledProof, [
        { lockHash: v.proofKey, shannons: BigInt(v.proofBalance) },
      ])
      expect(ok).toBe(true)
    })
  }
})
```

If vitest rejects the JSON import assertion syntax, switch to:
```ts
import { readFileSync } from 'node:fs'
const vectors = JSON.parse(readFileSync(new URL('./vectors/smt-vectors.json', import.meta.url), 'utf8'))
```

- [ ] **Step 4: Run to verify it passes**

Run: `cd ~/community-pool/packages/pool-core && npm test`
Expected: PASS — original 4 tests + 4 vector tests (2 cases × root+proof).

- [ ] **Step 5: Commit**

```bash
cd ~/community-pool
git add packages/pool-core/scripts/gen-vectors.ts \
  packages/pool-core/test/vectors/smt-vectors.json \
  packages/pool-core/test/smt.test.ts
git commit -m "test(pool-core): canonical SMT vectors + consumption tests"
```

---

## Task 6: Native Rust parity test against the shared vectors

This proves the Rust crate (the same code the on-chain lock will use) agrees with the wasm-generated vectors — the bridge that the future `pool-payout-lock` plan inherits.

**Files:**
- Create: `packages/pool-smt/tests/vectors.rs`
- Symlink or copy the vector file so the Rust test can read it.

- [ ] **Step 1: Point the Rust test at the shared vectors**

Create `packages/pool-smt/tests/vectors.rs`:
```rust
//! Parity test: the Rust SMT (same crate the on-chain lock uses) must reproduce
//! the roots and verify the proofs in the wasm-generated canonical vectors.

use serde::Deserialize;
use std::fs;

// Re-expose the crate's inner fns for the integration test.
use pool_smt as _;

#[derive(Deserialize)]
struct Entry {
    #[serde(rename = "lockHash")]
    lock_hash: String,
    shannons: String,
}

#[derive(Deserialize)]
struct Vector {
    name: String,
    entries: Vec<Entry>,
    root: String,
    #[serde(rename = "proofKey")]
    proof_key: String,
    #[serde(rename = "compiledProof")]
    compiled_proof: String,
    #[serde(rename = "proofBalance")]
    proof_balance: String,
}

fn load() -> Vec<Vector> {
    let path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../pool-core/test/vectors/smt-vectors.json"
    );
    let raw = fs::read_to_string(path).expect("vectors file present (run npm run gen-vectors)");
    serde_json::from_str(&raw).expect("valid vectors json")
}

#[test]
fn rust_reproduces_wasm_vectors() {
    for v in load() {
        let keys: Vec<String> = v.entries.iter().map(|e| e.lock_hash.clone()).collect();
        let bals: Vec<String> = v.entries.iter().map(|e| e.shannons.clone()).collect();

        let root = pool_smt::smt::root_inner(&keys, &bals).unwrap();
        assert_eq!(root, v.root, "root mismatch for {}", v.name);

        let ok = pool_smt::smt::verify_inner(
            &v.root,
            &v.compiled_proof,
            &[v.proof_key.clone()],
            &[v.proof_balance.clone()],
        )
        .unwrap();
        assert!(ok, "proof failed to verify for {}", v.name);
    }
}
```

- [ ] **Step 2: Expose the modules for integration tests**

In `packages/pool-smt/src/lib.rs`, change the module declarations so integration tests can reach them:
```rust
pub mod balance;
pub mod smt;
```
(was `mod balance; mod smt;`). Re-run `cargo build` to confirm no errors.

- [ ] **Step 3: Run to verify it passes**

Run: `cd ~/community-pool/packages/pool-smt && cargo test --test vectors`
Expected: PASS — `rust_reproduces_wasm_vectors`. This is the cross-impl parity proof: wasm-side roots/proofs are reproduced and verified by the native Rust crate.

- [ ] **Step 4: Commit**

```bash
cd ~/community-pool
git add packages/pool-smt/src/lib.rs packages/pool-smt/tests/vectors.rs
git commit -m "test(pool-smt): native parity against wasm-generated vectors"
```

---

## Final verification

- [ ] **Run the whole suite**

```bash
cd ~/community-pool/packages/pool-smt && cargo test
cd ~/community-pool/packages/pool-core && npm test
```
Expected: all Rust tests (balance + smt unit tests + vectors integration test) pass; all vitest tests pass.

- [ ] **Confirm the deliverables exist**

```bash
ls ~/community-pool/packages/pool-smt/pkg/pool_smt_bg.wasm
ls ~/community-pool/packages/pool-core/test/vectors/smt-vectors.json
```
Both present.

---

## What this unblocks

- **Part B — `pool-payout-lock` (Rust on-chain):** depends on `vendor/sparse-merkle-tree` (with the `smtc` feature for CKB-VM) + the same `BalanceValue`/key encoding from `balance.rs`, and will validate against `smt-vectors.json` to prove on-chain/off-chain parity.
- **Part C — `treasury-cell` state machine (TS):** imports `accrualRoot`/`genProof`/`verifyProof` from `pool-core` to compute treasury transitions and build settlement proofs.

## Self-review notes (done)

- **Spec coverage:** implements the "on-chain committed accrual root" data primitive (spec §2, §5 accrual integrity, §11 "Merkle scheme for accrued_root"). Threshold/fee/settlement logic are explicitly out of scope for this part and named in the unblocks section.
- **Placeholders:** none — every step has runnable code/commands and expected output.
- **Type consistency:** `accrualRoot`/`genProof`/`verifyProof` (TS) and `root_inner`/`gen_proof_inner`/`verify_inner` (Rust) names are consistent across Tasks 2–6; `AccrualEntry { lockHash, shannons }` shape is identical in wrapper, tests, and generator; `BalanceValue` LE-u64 encoding is defined once in `balance.rs` and reused.
- **Toolchain caveats flagged inline:** the two real risk points (vendored crate `compile`/`verify` signatures; wasm `init` export shape) have explicit "if it differs, check X" fallbacks rather than assuming.
