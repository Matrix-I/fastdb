# PostgreSQL release measurements

Use a disposable PostgreSQL 18 database named `fastdb_perf` on loopback.
Its user must own the database and be able to analyze its catalogs. Run
`setup.sql` once in a new database. It creates 20 schemas, 2,000 tables,
10,000 columns and 2,000 primary keys. Use a dedicated instance.

Create `fastdb_perf` on the dedicated instance as `postgres` before setup.
Supply `PGPORT` and `PGPASSWORD` through the fixture environment. The same
loopback endpoint and user are required for setup and measurement.

```sh
createdb -h 127.0.0.1 -U postgres fastdb_perf
psql -h 127.0.0.1 -U postgres -X -v ON_ERROR_STOP=1 -d fastdb_perf -f benchmarks/postgres/setup.sql
cargo build --locked --release -p db-wire --example postgres_performance
mkdir -p benchmarks/results
target/release/examples/postgres_performance > benchmarks/results/v0.2.0-postgres.json
```

Each trial opens a connection, pings, lists tables, then joins the five additional
metadata readers used when a connection opens. One warmup is discarded and five
samples are retained. This measures Navigator backend loading. Tree rendering
requires desktop evidence.

The mixed query streams 20,000 rows with NULLs and variable text, including a
first row with 20,000-byte text and binary cells. Read-back validates early and
final pages. A separate probe tests 300,000-byte text and binary cells. Capacity
rejection produces JSON and a nonzero exit status. Each trial has a 60-second
deadline and the oversized probe has a 15-second deadline.

Run `EXPLAIN (ANALYZE, BUFFERS)` on `mixed.sql` and `large-lob.sql` and retain
plans alongside the report. The bounded generated series uses a function scan.
After measurement, remove the disposable database or container. Preserve JSON
and logs before cleaning temporary files. `benchmarks/results` is Git-ignored.
Publish the summary and failures in `benchmarks/VALIDATION.md`.

## Cold startup

Stage and hash the executable and prepare an empty profile before reboot.
Record OS, hardware and display configuration. After reboot and login, measure
the first launch with a monotonic launch-to-first-`FASTDB_BENCH ready` probe,
a 10-second deadline, and child kill/reap cleanup. Save the event and duration.
Run this before FastDB or the warm suite. Protocol v1 discards its first run.
Cold startup remains pending if this first-launch evidence is missing.
Arrange the reboot with the release-host operator because it ends the desktop
session.

On Linux, stage an absolute executable path and an empty profile on persistent
storage before reboot. Then run this probe once, supplying those paths. It
records failure with a nonzero exit code if no valid first-frame event arrives.

```sh
python3 - /absolute/path/fastdb /absolute/path/empty-profile <<'PY'
import json
import os
import selectors
import subprocess
import sys
import time

env = dict(os.environ, FASTDB_CONFIG_DIR=sys.argv[2], RUST_LOG="warn")
started = time.monotonic()
child = subprocess.Popen([sys.argv[1], "--benchmark"], env=env,
                         cwd=sys.argv[2], stdout=subprocess.PIPE)
try:
    os.set_blocking(child.stdout.fileno(), False)
    pending = b""
    with selectors.DefaultSelector() as selector:
        selector.register(child.stdout, selectors.EVENT_READ)
        while time.monotonic() - started < 10:
            if not selector.select(timeout=max(0, 10 - (time.monotonic() - started))):
                break
            chunk = os.read(child.stdout.fileno(), 4096)
            if not chunk:
                break
            pending += chunk
            if len(pending) > 65536:
                sys.exit("cold startup output exceeded 64 KiB")
            while b"\n" in pending:
                line, pending = pending.split(b"\n", 1)
                if line.startswith(b"FASTDB_BENCH "):
                    event = json.loads(line.removeprefix(b"FASTDB_BENCH "))
                    if event.get("event") == "ready":
                        assert event["protocol"] == 1 and not event["debug_assertions"]
                        print(json.dumps({"startup_ms": (time.monotonic() - started) * 1000,
                                          "ready": event}))
                        sys.exit(0)
        sys.exit("cold startup failed or exceeded 10 seconds")
finally:
    if child.poll() is None:
        child.kill()
    child.wait(timeout=5)
PY
```
