//! Community-pool accrual SMT: balance key->value root + Merkle proofs.
//! Pure logic lives in `balance` and `smt`; this file is the wasm surface.

pub mod balance;
pub mod smt;

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
