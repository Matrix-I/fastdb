"""Exercise the documented cold probe against real subprocess boundaries."""
import os
from pathlib import Path
import signal
import subprocess
import sys
import tempfile
import unittest


class ColdProbeTests(unittest.TestCase):
    def run_probe(self, program, short_deadline=False):
        readme = Path(__file__).with_name("README.md").read_text()
        source = readme.split("<<'PY'\n", 1)[1].split("\nPY\n", 1)[0]
        if short_deadline:
            # Only the timeout case needs a shortened production deadline.
            source = source.replace("< 10:", "< 0.1:")
            source = source.replace("max(0, 10 -", "max(0, 0.1 -")
        with tempfile.TemporaryDirectory() as directory:
            executable = Path(directory) / "fixture"
            executable.write_text(f"#!{sys.executable}\n{program}\n")
            executable.chmod(0o700)
            child = subprocess.Popen(
                [sys.executable, "-", str(executable), directory],
                stdin=subprocess.PIPE,
                stdout=subprocess.PIPE,
                stderr=subprocess.PIPE,
                text=True,
                start_new_session=True,
            )
            try:
                output, error = child.communicate(source, timeout=3)
                return child.returncode, output, error
            finally:
                # Also clean up a fixture if a regression blocks the probe.
                try:
                    os.killpg(child.pid, signal.SIGKILL)
                except ProcessLookupError:
                    pass
                child.wait(timeout=3)

    def test_partial_line_cannot_bypass_deadline(self):
        status, _, error = self.run_probe(
            "import os, signal\nos.write(1, b'partial')\nsignal.pause()",
            short_deadline=True,
        )
        self.assertNotEqual(status, 0)
        self.assertIn("cold startup failed or exceeded", error)

    def test_ready_event_is_captured(self):
        status, output, error = self.run_probe(
            'print(\'FASTDB_BENCH {"event":"ready","protocol":1,'
            '"debug_assertions":false}\', flush=True)'
        )
        self.assertEqual(status, 0, error)
        self.assertIn('"startup_ms":', output)

    def test_debug_binary_is_rejected(self):
        status, _, _ = self.run_probe(
            'print(\'FASTDB_BENCH {"event":"ready","protocol":1,'
            '"debug_assertions":true}\', flush=True)'
        )
        self.assertNotEqual(status, 0)


if __name__ == "__main__":
    unittest.main()
