'use strict';

const path = require('node:path');
const { bindingFileName } = require('./platform');
const bindingPath = path.join(__dirname, bindingFileName());

try {
  module.exports = require(bindingPath);
} catch (error) {
  error.message = `Unable to load @ruflo/native for this platform (${process.platform}-${process.arch}). Build it with scripts/build-napi.sh or install the matching release archive. ${error.message}`;
  throw error;
}
