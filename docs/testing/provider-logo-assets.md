# Provider logo asset verification

## Reproduction

Base commit: `88e124c`, with the current uncommitted two-step connection UI.
Environment: macOS, Rust `1.99.1 (58cf8f9c3 2026-10-13)`.

Open Add new connection and inspect PostgreSQL, MySQL and SQLite cards on Retina.
The user screenshot shows blurred logos. Each original embedded PNG is only
16 × 16 pixels, while the picker displays 44 logical pixels (88 physical pixels
at 2×). Expected: crisp provider logos. Actual: enlarged low-resolution bitmaps.
The existing picker code and PNG IHDR dimensions confirm the cause.

## Change and focused verification

Replaced those assets with unmodified Devicon original SVGs and updated picker,
navigator and asset tests. Removed the now-unused PNGs. Each SVG has a 128 × 128
canvas with vector paths; the SQLite asset also uses a local gradient. No bitmap,
script, external image or font is required. Attribution, source links, checksums
and the upstream MIT notice are in `assets/icons/`.

`cargo test -p fastdb-app provider_card_logos_are_scalable_vector_artwork`
failed against the old PostgreSQL PNG with:

```text
icons/postgresql.png: raster logo cannot scale cleanly from 16px to 88px Retina cards
```

After replacement, `cargo test -p fastdb-app assets::tests` passed both tests.
The regression test checks embedded data, vector paths and absence of embedded
bitmaps; the existing asset-load test covers the three new paths.

Each source SVG was parsed with Python ElementTree and rendered with
`rsvg-convert -w 256 -h 256`. All three rendered successfully and their PNG
previews were visually inspected: clean outlines, transparent backgrounds and
recognizable provider marks. GPUI 0.2.2's existing `img` loader rasterizes these
canvases at 2× (256 × 256), sufficient for the 88px Retina picker icons.

Independent review found no actionable issues in assets, paths, rendering
compatibility, attribution or the regression test.

## Self-review

| Item | Concrete reason / limit |
| --- | --- |
| Requirements and correctness | All three reported logos now use vector artwork in both picker and navigator; no obsolete PNG references remain. |
| Performance | Three small local SVGs total about 14 KiB; GPUI uses its existing cached image loader, with no network work added. |
| Security | SVGs contain only paths and a local gradient; no scripts, external resources or secrets. |
| Robustness and dependencies | Embedded assets load in tests; no package added or upgraded, and existing error handling is unchanged. |
| Syntax and naming | English source names and the project's existing icon path/rendering conventions are retained. |
| Modern APIs | Uses existing GPUI `img` and `AssetSource` APIs without introducing deprecated calls. |
| Business clarity | Only asset quality changes; provider availability, layout and connection behavior are unchanged. |
| Test coverage | Regression reproduced before the fix; embedded asset tests pass and all SVGs render in librsvg. Actual GPUI/Retina appearance still needs manual acceptance. |
| Design | Existing shared image loading and provider icon selection remain in place; no new abstraction added. |

## Workspace verification

- `cargo fmt --all -- --check`: passed.
- `git diff --check`: passed.
- `cargo test --workspace`: 590 passed, zero failures, including concurrent
  working-tree changes outside this logo task. Existing SSH socket tests ran
  with local socket access enabled.
- `cargo build -p fastdb-app`: passed; current runnable binary retained.
- `cargo clippy --workspace --all-targets`: first attempt aborted while checking
  the app test target with `fatal runtime error: stack overflow`. Retried as
  `RUST_MIN_STACK=67108864 cargo clippy --workspace --all-targets`: passed.
  An intermediate warning from an unrelated title-bar edit was absent in this
  final successful pass. No source change was made for the compiler stack limit.
- `FASTDB_CONFIG_DIR=/tmp/fastdb-logo-smoke FASTDB_LOCAL_CREDENTIALS=1 ./target/debug/fastdb`:
  binary started and remained running until intentionally stopped. The picker
  itself was not exercised automatically, as Computer Use is prohibited.

## Manual acceptance still pending

Run the rebuilt app and check all three provider cards and their navigator icons
on Retina, including light/dark themes. No Computer Use was performed, per the
project preference; actual GUI appearance has not been accepted automatically.
