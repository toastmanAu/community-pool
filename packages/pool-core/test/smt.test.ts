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
