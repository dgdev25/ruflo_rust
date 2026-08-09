'use strict';
const assert = require('node:assert/strict');
const test = require('node:test');
const { bindingFileName } = require('../platform');

test('selects the release addon for every automated native target', () => {
  assert.equal(bindingFileName('linux', 'x64'), 'ruflo-native.linux-x64-gnu.node');
  assert.equal(bindingFileName('linux', 'arm64'), 'ruflo-native.linux-arm64-gnu.node');
  assert.equal(bindingFileName('darwin', 'x64'), 'ruflo-native.darwin-x64.node');
  assert.equal(bindingFileName('darwin', 'arm64'), 'ruflo-native.darwin-arm64.node');
  assert.equal(bindingFileName('win32', 'x64'), 'ruflo-native.win32-x64-msvc.node');
});

test('rejects a platform without a released addon', () => {
  assert.throws(() => bindingFileName('win32', 'arm64'), /Unsupported/);
});
