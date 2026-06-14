//! Parity test: the Rust SMT (same crate the on-chain lock uses) must reproduce
//! the roots and verify the proofs in the wasm-generated canonical vectors.

use serde::Deserialize;
use std::fs;

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
