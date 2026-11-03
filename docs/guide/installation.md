# Installation

## Install the macOS preview

Download the Apple Silicon DMG or ZIP from [GitHub Releases](https://github.com/Matrix-I/fastdb/releases/latest) and copy FastDB.app into `/Applications`.
The v0.1.0 release is an early preview for macOS Apple Silicon; Linux packages are not included.
The app is ad-hoc signed and not notarized. If Gatekeeper blocks it, run:

```bash
xattr -dr com.apple.quarantine /Applications/FastDB.app
```

The published v0.1.0 preview stores saved passwords in plaintext JSON. Current source builds use the OS keychain and migrate existing passwords. Performance figures in [Performance goals](../development/performance-goals.md) remain unmeasured targets.
