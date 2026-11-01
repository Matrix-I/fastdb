# Rust compatibility and macOS packaging warnings

## Goal

Remove the future-incompatibility warnings from `block 0.1.6` and
`proc-macro-error2 2.0.1`, and avoid deprecated DMG creation on newer macOS.

## Implementation

1. Vendor the two affected crates and apply narrow source fixes through
   `[patch.crates-io]`: use an inhabited opaque FFI type for the block class and
   make the re-exported `proc_macro` crate public. Preserve upstream versions and
   licenses; document why the patches exist and when to remove them.
2. Share DMG creation between the local and CI packaging scripts. Detect support
   for `diskutil image create from`, create a compressed UDZO image with the
   FastDB volume name, and retain `hdiutil` for older macOS. Propagate actual
   creation failures rather than retrying them through the deprecated command.
3. Document packaging behavior without changing unrelated working-tree edits.

## Validation

Run shell syntax checks and isolated command-selection/error-propagation tests.
Create and inspect a small real DMG on this Mac. Run `cargo fmt --all`,
`cargo clippy --workspace --all-targets`, and `cargo test --workspace`; inspect
fresh future-incompatibility output. No UI behavior changes are planned.

