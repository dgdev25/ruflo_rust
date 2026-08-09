'use strict';

const path = require('node:path');
const bindingPath = path.join(__dirname, 'ruflo-native.linux-x64-gnu.node');

try {
  module.exports = require(bindingPath);
} catch (error) {
  error.message = `Unable to load @ruflo/native for this platform (${process.platform}-${process.arch}). Build it with scripts/build-napi.sh. ${error.message}`;
  throw error;
}
