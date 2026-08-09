# `ruflo-core` consumer

This is a minimal standalone Rust application that depends on the reusable
`ruflo-core` crate by local path. It deliberately uses no CLI, process state,
or Node runtime.

Run it from the repository root:

```bash
cargo run --manifest-path examples/core-adapter/Cargo.toml
```

For a fork or another local checkout, update the path dependency in
`Cargo.toml`. The crates are not yet published to crates.io, so this is the
supported integration model today.
