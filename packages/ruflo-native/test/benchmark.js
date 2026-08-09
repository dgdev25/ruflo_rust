'use strict';
const native = require('..');
const iterations = Number.parseInt(process.argv[2] ?? '30', 10);
const fixture = 'native addon parity benchmark fixture';
const dimensions = 384;

function fnv1a(value) {
  let hash = 0xcbf29ce484222325n;
  for (const byte of Buffer.from(value)) hash = (hash ^ BigInt(byte)) * 0x100000001b3n & 0xffffffffffffffffn;
  return hash;
}

function fingerprint(vector) {
  let hash = 0xcbf29ce484222325n;
  for (const byte of new Uint8Array(new Float64Array(vector).buffer)) hash = (hash ^ BigInt(byte)) * 0x100000001b3n & 0xffffffffffffffffn;
  return hash.toString(16).padStart(16, '0');
}

// Literal JavaScript port of ruflo-core's deterministic feature-hash embedder.
// The benchmark fixture is ASCII, so this compares one algorithm and one result
// rather than unrelated semantic models.
function javascriptEmbed(text, dimensions) {
  const vector = new Array(dimensions).fill(0);
  for (const token of text.toLowerCase().split(/[^a-z0-9]+/).filter(Boolean)) {
    for (const feature of [token, ...Array.from({ length: Math.max(0, token.length - 2) }, (_, index) => token.slice(index, index + 3))]) {
      const first = fnv1a(feature);
      const second = fnv1a(`ruflo:${feature}`);
      const index = Number(first % BigInt(dimensions));
      vector[index] += second & 1n ? -1 : 1;
    }
  }
  const norm = Math.sqrt(vector.reduce((sum, value) => sum + value * value, 0));
  return vector.map(value => norm > 0 ? value / norm : value);
}

const output = native.embed(fixture, dimensions);
const javascript = javascriptEmbed(fixture, dimensions);
if (fingerprint(javascript) !== fingerprint(output.vector)) throw new Error('JavaScript reference does not match napi-rs result');
const samples_ns = [];
const javascript_samples_ns = [];
for (let index = 0; index < iterations; index += 1) {
  const started = process.hrtime.bigint();
  const answer = native.embed(fixture, dimensions);
  samples_ns.push(Number(process.hrtime.bigint() - started));
  if (JSON.stringify(answer.vector) !== JSON.stringify(output.vector)) throw new Error('addon result changed during benchmark');

  const javascriptStarted = process.hrtime.bigint();
  const javascriptAnswer = javascriptEmbed(fixture, dimensions);
  javascript_samples_ns.push(Number(process.hrtime.bigint() - javascriptStarted));
  if (fingerprint(javascriptAnswer) !== fingerprint(output.vector)) throw new Error('JavaScript result changed during benchmark');
}
process.stdout.write(JSON.stringify({ provider: output.provider, dimensions: output.dimensions, fingerprint: fingerprint(output.vector), samples_ns, javascript_samples_ns }));
