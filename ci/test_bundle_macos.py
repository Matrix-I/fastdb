"""Exercise bundle orchestration with real checksums/minisign and fake build tools."""
import hashlib
import os
from pathlib import Path
import shutil
import subprocess
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[1]
MINISIGN = shutil.which("minisign")


class BundleTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory(prefix="fastdb-bundle-")
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        self.bin = self.root / "bin"
        self.bin.mkdir()
        (self.root / "resources").mkdir()
        (self.root / "ci").mkdir()
        shutil.copy(ROOT / "bundle_macos.sh", self.root)
        shutil.copy(ROOT / "ci/create-macos-dmg.sh", self.root / "ci")
        (self.root / "Cargo.toml").write_text('[workspace.package]\nversion = "0.1.0"\n')
        (self.root / "resources/FastDB.icns").write_bytes(b"icon")
        binary = self.root / "target/aarch64-apple-darwin/release/fastdb"
        binary.parent.mkdir(parents=True)
        binary.write_bytes(b"release binary fixture")
        self.stub("uname", 'if [ "${1:-}" = -s ]; then echo Darwin; else echo arm64; fi')
        self.stub("rustc", 'echo "host: aarch64-apple-darwin"')
        self.stub("cargo", 'exit "${TEST_BUILD_EXIT:-0}"')
        self.stub("codesign", 'exit 0')
        self.stub("diskutil", 'if [ "${4:-}" = --help ]; then exit 0; fi\nfor arg in "$@"; do last="$arg"; done\nprintf dmg > "$last"')
        self.stub("hdiutil", 'for arg in "$@"; do last="$arg"; done\nprintf dmg > "$last"')
        self.env = dict(os.environ, PATH=f"{self.bin}:{Path.home() / '.cargo/bin'}:{os.environ['PATH']}",
                        SDKROOT="/fixture-sdk", CARGO_TARGET_DIR=str(self.root / "target"),
                        MINISIGN_SECRET_KEY=str(self.root / "absent.key"))
        self.env.pop("FASTDB_MINISIGN_PUBKEY", None)

    def stub(self, name, body):
        path = self.bin / name
        path.write_text("#!/bin/bash\nset -eu\n" + body + "\n")
        path.chmod(0o755)

    def run_bundle(self, success=True):
        result = subprocess.run(["/bin/bash", str(self.root / "bundle_macos.sh"), "--no-install"],
                                env=self.env, capture_output=True, text=True, timeout=30)
        self.assertEqual(result.returncode == 0, success, result.stdout + result.stderr)
        return result

    def key(self, name):
        if not MINISIGN:
            self.fail("minisign is required for signing tests")
        secret = self.root / f"{name}.key"
        public = self.root / f"{name}.pub"
        subprocess.run([MINISIGN, "-G", "-W", "-s", str(secret), "-p", str(public)],
                       check=True, capture_output=True, timeout=10)
        return secret, public

    def assert_checksums(self):
        sums = self.root / "dist/SHA256SUMS.txt"
        entries = [line.split("  ") for line in sums.read_text().splitlines()]
        self.assertEqual([name for _, name in entries],
                         ["FastDB.dmg", "FastDB-macos-aarch64.zip", "fastdb-macos-aarch64"])
        for digest, name in entries:
            self.assertEqual(digest, hashlib.sha256((sums.parent / name).read_bytes()).hexdigest())
        return sums

    def test_unsigned_build_clears_stale_signature_and_checksums_artifacts(self):
        dist = self.root / "dist"
        dist.mkdir()
        (dist / "SHA256SUMS.txt.minisig").write_text("stale")
        result = self.run_bundle()
        self.assertIn("left unsigned", result.stderr)
        self.assert_checksums()
        self.assertFalse((dist / "SHA256SUMS.txt.minisig").exists())

    def test_signed_build_verifies_with_project_key(self):
        secret, public = self.key("release")
        shutil.copy(public, self.root / "resources/minisign.pub")
        self.env["MINISIGN_SECRET_KEY"] = str(secret)
        self.run_bundle()
        sums = self.assert_checksums()
        subprocess.run([MINISIGN, "-V", "-q", "-p", str(public), "-m", str(sums)],
                       check=True, capture_output=True, timeout=10)

    def test_missing_minisign_leaves_unsigned_checksums(self):
        self.env["PATH"] = f"{self.bin}:/usr/bin:/bin:{Path.home() / '.cargo/bin'}"
        self.assertIn("minisign not installed", self.run_bundle().stderr)
        self.assert_checksums()

    def test_failed_signing_removes_partial_signature(self):
        secret, public = self.key("release")
        self.env["MINISIGN_SECRET_KEY"] = str(secret)
        self.env["FASTDB_MINISIGN_PUBKEY"] = public.read_text().splitlines()[-1]
        self.stub("minisign", 'for arg in "$@"; do last="$arg"; done\nprintf partial > "$last.minisig"\nexit 1')
        self.assertIn("signing failed", self.run_bundle(False).stderr)
        self.assertFalse((self.root / "dist/SHA256SUMS.txt.minisig").exists())

    def test_mismatched_key_fails_and_removes_signature(self):
        secret, _ = self.key("signer")
        _, public = self.key("other")
        self.env["MINISIGN_SECRET_KEY"] = str(secret)
        self.env["FASTDB_MINISIGN_PUBKEY"] = public.read_text().splitlines()[-1]
        self.assertIn("does not match", self.run_bundle(False).stderr)
        self.assertFalse((self.root / "dist/SHA256SUMS.txt.minisig").exists())

    def test_signing_requires_public_key(self):
        secret, _ = self.key("release")
        self.env["MINISIGN_SECRET_KEY"] = str(secret)
        self.assertIn("before signing", self.run_bundle(False).stderr)

    def test_failed_build_removes_stale_release_metadata(self):
        dist = self.root / "dist"
        dist.mkdir()
        for name in ["SHA256SUMS.txt", "SHA256SUMS.txt.minisig"]:
            (dist / name).write_text("stale")
        self.env["TEST_BUILD_EXIT"] = "1"
        self.run_bundle(False)
        self.assertFalse((dist / "SHA256SUMS.txt").exists())
        self.assertFalse((dist / "SHA256SUMS.txt.minisig").exists())


if __name__ == "__main__":
    unittest.main()
