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
