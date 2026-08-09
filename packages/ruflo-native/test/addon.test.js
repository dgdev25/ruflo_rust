'use strict';
const assert = require('node:assert/strict');
const test = require('node:test');
const native = require('..');

test('addon exposes deterministic core embedding', () => {
  const answer = native.embed('native parity contract', 32);
  assert.equal(answer.dimensions, 32);
  assert.equal(answer.provider, 'deterministic-feature-hash-v1');
  assert.equal(answer.vector.length, 32);
});

test('addon exposes typed routing', () => {
  assert.equal(native.route('optimise benchmark performance', ['coder', 'optimizer']).agent, 'optimizer');
});
