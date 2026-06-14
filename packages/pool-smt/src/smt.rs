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
