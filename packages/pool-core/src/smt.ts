import init, {
  accrual_root,
  gen_proof,
  verify_proof,
} from 'pool-smt'
import { readFile } from 'node:fs/promises'
import { createRequire } from 'node:module'

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
    const require = createRequire(import.meta.url)
    const mainPath = require.resolve('pool-smt')           // .../pool_smt.js
    const wasmPath = new URL('pool_smt_bg.wasm', `file://${mainPath}`)
    const bytes = await readFile(wasmPath)
    await init({ module_or_path: bytes })
    ready = true
  }
}

function split(entries: AccrualEntry[]): { keys: string[]; balances: string[] } {
  return {
    keys: entries.map((e) => e.lockHash),
    balances: entries.map((e) => e.shannons.toString()),
  }
}

export function accrualRoot(entries: AccrualEntry[]): string {
  const { keys, balances } = split(entries)
  return accrual_root(keys, balances)
}

export function genProof(entries: AccrualEntry[], proofKeys: string[]): string {
  const { keys, balances } = split(entries)
  return gen_proof(keys, balances, proofKeys)
}

export function verifyProof(
  root: string,
  compiledProof: string,
  leaves: AccrualEntry[],
): boolean {
  const keys = leaves.map((l) => l.lockHash)
  const balances = leaves.map((l) => l.shannons.toString())
  return verify_proof(root, compiledProof, keys, balances)
}
