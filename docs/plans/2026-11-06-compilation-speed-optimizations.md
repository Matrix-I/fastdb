# Build and Compilation Speed Optimizations

## Goal

Track and implement remaining compilation performance improvements for FastDB to minimize inner-loop build times (`cargo build`, `cargo test`, `cargo run`) across macOS and Linux development environments and CI pipelines.

---

## Current Baseline

The following compilation optimizations are already in place:
- **Multi-crate Workspace**: Cleanly partitioned into 8 modular crates (`platform-hal`, `mem-engine`, `sql-intel`, `db-wire`, `ui-core`, `ui-editor`, `ui-grid`, `fastdb-app`) plus `xtask`, maximizing parallel compilation and incremental rebuild caching.
- **Dynamic Dispatch over Generics**: Database drivers and sessions use `Arc<dyn DatabaseDriver>` rather than monomorphized generics, preventing LLVM code duplication across the UI and session layers.
- **Lean Cargo Features**: Strict `default-features = false` and selective feature flags on heavy crates (`tokio`, `chrono`, `zed-reqwest`, `lz4_flex`, `odbc-api`). `serde_derive` is excluded from core engine crates.
- **Dev Profile Tuning**: Root `Cargo.toml` sets `debug = "line-tables-only"` for workspace crates and `debug = false` + `opt-level = 3` for external dependencies (`[profile.dev.package."*"]`).
- **CI Cache**: GitHub Actions uses `Swatinem/rust-cache@v2` for caching Cargo registry and build artifacts.

---

## Remaining Optimization Points to Implement

### 1. Split Debuginfo in Dev Profile (`split-debuginfo = "unpacked"`)
- **Problem**: On macOS and Linux, the linker spends considerable time assembling full debug symbols into single DWARF / dSYM bundles during debug builds.
- **Action**:
  - Update `[profile.dev]` in root `Cargo.toml`:
    ```toml
    [profile.dev]
    opt-level = 1
    debug = "line-tables-only"
    split-debuginfo = "unpacked"
    ```
  - Verify backtraces still display file and line numbers properly.
  - Measure link time improvements on macOS and Linux.

---

### 2. Alternative Fast Linkers (`mold` and `lld`)
- **Problem**: Default system linkers (`ld64` on macOS, `ld` on Linux) are single-threaded or slow when resolving symbols for large binaries like `fastdb-app`.
- **Action**:
  - **Linux (`mold`)**:
    - Add target configuration for `x86_64-unknown-linux-gnu` in `.cargo/config.toml`:
      ```toml
      [target.x86_64-unknown-linux-gnu]
      linker = "clang"
      rustflags = ["-C", "link-arg=-fuse-ld=mold"]
      ```
    - Update CI and documentation (`docs/guide/` or `CLAUDE.md`) on installing `mold`.
  - **macOS (`lld`)**:
    - Benchmark linking `fastdb` with `lld` (e.g. via `brew install llvm` / `rustflags = ["-C", "link-arg=-fuse-ld=lld"]`) versus Apple `ld64`.
    - If stable and demonstrably faster, provide instructions or sample configuration in `.cargo/config.toml.example`.

---

### 3. Compilation Caching with `sccache`
- **Problem**: Clean builds, branch switches, or toolchain updates recompile the entire dependency tree from scratch on local machines.
- **Action**:
  - Provide documented guidelines or an opt-in entry in `.cargo/config.toml`:
    ```toml
    [build]
    # rustc-wrapper = "sccache"
    ```
  - Add developer documentation on installing and running `sccache` daemon (`cargo install sccache` / `brew install sccache`).
  - Evaluate combining `sccache` in CI with GitHub Actions cache backend for cross-PR artifact sharing.

---

### 4. Cranelift Backend for Local Debug Builds (`rustc_codegen_cranelift`)
- **Problem**: LLVM code generator performs heavy optimization passes even in debug mode, dominating compilation time during rapid edit-run cycles.
- **Action**:
  - Explore opt-in support for `rustc_codegen_cranelift` for developers iterating purely on UI or application logic.
  - Document toolchain setup: requires nightly toolchain (`rustup toolchain install nightly`, `rustup component add rustc-codegen-cranelift-preview --toolchain nightly`).
  - Add an alias or `xtask` subcommand (e.g. `cargo xtask cranelift-run`) for one-command test runs with Cranelift.

---

### 5. Build Timings and Regression Profiling
- **Problem**: New dependencies or macro expansions can silently inflate compile times without immediate visibility.
- **Action**:
  - Add an `xtask` profile command or document `cargo build --timings` usage in developer docs.
  - Run `--timings` analysis to inspect bottleneck crates and crate graph critical paths.
  - Set a review rule to check compilation impact when adding new external dependencies or proc-macro crates.

---

### 6. Inner Non-Generic Function Pattern for Future Generic Utilities
- **Problem**: As utility functions or helper libraries expand, generic functions with heavy logic create redundant monomorphized code.
- **Action**:
  - Establish coding convention: functions with generic constraints (e.g. `T: AsRef<[u8]>` or `T: AsRef<Path>`) should delegate complex internal logic to private non-generic inner functions (`fn inner(slice: &[u8])`).
  - Apply this convention during code reviews for new generic functions in `mem-engine`, `db-wire`, and `ui-grid`.

---

## Verification Plan

1. **Profile Benchmark**: Measure `cargo check`, incremental `cargo build`, and full `cargo build` times before and after applying `split-debuginfo = "unpacked"`.
2. **Linker Verification**: Ensure macOS bundle (`./bundle_macos.sh`) and Linux CI builds succeed and binaries execute correctly with alternative linkers.
3. **Traceability**: Keep CI stable; all opt-in tools (`sccache`, `cranelift`) must not break default `cargo build` on pristine developer machines.
