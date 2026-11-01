# Dependency patches

These crates are excluded from the workspace and selected via `[patch.crates-io]`.
Keep patches narrow; remove each when its callers can use an upstream fix.

- `xattr 0.2.3`: existing Linux compatibility patch; see the root `Cargo.toml`.
- `block 0.1.6`: copied from the crates.io source. Replace the uninhabited opaque
  `Class` enum with an inhabited `#[repr(C)]` opaque struct. Only its address is
  used; the block layout and runtime symbol remain unchanged. This fixes
  `uninhabited_static` (rust-lang/rust#74840). Spell out the existing C ABI on
  extern declarations to fix `missing_abi` warnings exposed by the local patch.
  The packaged dev-dependency's
  nonexistent `test_utils` path is removed; its registry version is retained.
  Upstream: <https://github.com/SSheldon/rust-block> (MIT).
- `proc-macro-error2 2.0.1`: copied from the crates.io source. Make
  `extern crate proc_macro` public to match its existing public re-export,
  fixing E0365 (rust-lang/rust#127909). Upstream:
  <https://github.com/GnomedDev/proc-macro-error-2> (MIT OR Apache-2.0);
  license files are included in the crate directory.
