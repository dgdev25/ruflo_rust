'use strict';

function bindingFileName(platform = process.platform, arch = process.arch) {
  const target = `${platform}-${arch}`;
  const suffix = {
    'linux-x64': 'linux-x64-gnu',
    'linux-arm64': 'linux-arm64-gnu',
    'darwin-x64': 'darwin-x64',
    'darwin-arm64': 'darwin-arm64',
    'win32-x64': 'win32-x64-msvc',
  }[target];

  if (!suffix) {
    throw new Error(`Unsupported @ruflo/native platform: ${target}`);
  }

  return `ruflo-native.${suffix}.node`;
}

module.exports = { bindingFileName };
