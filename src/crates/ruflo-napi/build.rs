fn main() {
    // Configure the platform-specific linker contract required by Node addons.
    // In particular, macOS must leave N-API symbols for Node to resolve when
    // the addon is loaded rather than trying to resolve them at build time.
    napi_build::setup();
}
