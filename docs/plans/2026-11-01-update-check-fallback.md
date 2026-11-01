# Reliable update checks and Litty alignment

## Goal

Keep update checks working when the unauthenticated GitHub API returns an error,
using the same release-page redirect fallback as Litty Terminal. Show the native
macOS up-to-date alert with the FastDB icon, app name, current version and OK button.

## Implementation

1. Keep API release metadata as the primary source. On request, status or parsing
   failure, follow the repository's public `releases/latest` redirect with bounded
   request timeouts. Validate the final repository URL and semantic version tag.
2. Compare fallback tags using the existing version and skipped-version logic.
   Do not invent installer URLs: offer View Release when metadata is unavailable.
   Preserve errors when both lookups fail; do not report a failed check as current.
3. Capture the originating window for manual checks and show the native GPUI
   platform prompt for successful up-to-date results on macOS. Keep the existing
   cross-platform dialog as a fallback. Defer the global menu handler's window
   update until action dispatch releases the window borrow.
4. Update README and add isolated local HTTP regression tests for API success,
   rate limits, redirect validation, fallback failure and skipped releases.

## Compact dialog follow-up

- Reduce About to 300 px with a 64 px icon, smaller typography and 16 px padding.
- Use a 380 px update card for short status messages and 480 px for release notes.
  Reduce status spacing and keep messages centered and within the card width.
- Shorten the native macOS confirmation text so AppKit can size it more compactly.
- Update README and rerun the workspace checks and app launch.

## Validation

Run `cargo fmt --all`, `cargo clippy --workspace --all-targets`, and
`cargo test --workspace`. Build and launch the app; verify native prompts if UI
automation permissions are available. Test networking without a live GitHub
dependency; use live read-only requests only as supplemental verification.
