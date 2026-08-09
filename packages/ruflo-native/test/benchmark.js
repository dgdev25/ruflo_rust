'use strict';
const native = require('..');
const iterations = Number.parseInt(process.argv[2] ?? '30', 10);
const output = native.embed('native addon parity benchmark fixture', 384);
const bytes = new Uint8Array(new Float64Array(output.vector).buffer);
let hash = 0xcbf29ce484222325n;
for (const byte of bytes) hash = (hash ^ BigInt(byte)) * 0x100000001b3n & 0xffffffffffffffffn;
const samples_ns = [];
for (let index = 0; index < iterations; index += 1) {
  const started = process.hrtime.bigint();
  const answer = native.embed('native addon parity benchmark fixture', 384);
  samples_ns.push(Number(process.hrtime.bigint() - started));
  if (JSON.stringify(answer.vector) !== JSON.stringify(output.vector)) throw new Error('addon result changed during benchmark');
}
process.stdout.write(JSON.stringify({ provider: output.provider, dimensions: output.dimensions, fingerprint: hash.toString(16).padStart(16, '0'), samples_ns }));
