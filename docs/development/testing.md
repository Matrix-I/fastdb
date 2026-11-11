# Testing

```bash
# Unit tests and SQLite driver tests (live-database tests skip themselves)
cargo test --workspace
```

The PostgreSQL and MySQL integration tests run when a server is configured through environment variables. They cover binary decoding against the server's own text output, incremental streaming, cancellation, and connections that outlive the caller's runtime.

| Variable | Default | Purpose |
| :--- | :--- | :--- |
| `FASTDB_TEST_PG_PORT` | (unset: skip) | PostgreSQL port; enables `crates/db-wire/tests/postgres_test.rs` |
| `FASTDB_TEST_PG_HOST` / `_USER` | `127.0.0.1` / `postgres` | PostgreSQL host and user (trust auth expected) |
| `FASTDB_TEST_MYSQL_PORT` | (unset: skip) | MySQL port; enables `crates/db-wire/tests/mysql_test.rs` |
| `FASTDB_TEST_MYSQL_HOST` / `_USER` / `_PASSWORD` / `_DATABASE` | `127.0.0.1` / `root` / none / `mysql` | MySQL connection |
| `FASTDB_TEST_KEYCHAIN` | unset | Set to `1` to run the native credential-store round trip with an isolated test service |
| `FASTDB_REQUIRE_DB_TESTS` | unset | When set, a missing port fails the tests instead of skipping them (used in CI) |

For example, with Docker:

```bash
docker run -d --name fastdb-pg -e POSTGRES_HOST_AUTH_METHOD=trust -p 5432:5432 postgres:16
```

```bash
docker run -d --name fastdb-mysql -e MYSQL_ROOT_PASSWORD=fastdb -p 3306:3306 mysql:8.4
```

```bash
FASTDB_TEST_PG_PORT=5432 FASTDB_TEST_MYSQL_PORT=3306 FASTDB_TEST_MYSQL_PASSWORD=fastdb cargo test -p db-wire
```

## TLS integration tests

With Docker and OpenSSL available, run `./ci/test-tls.sh`. It creates disposable PostgreSQL and MySQL servers with a private test CA, checks encrypted sessions, rejects untrusted certificates and incorrect hostnames, verifies PostgreSQL TLS cancellation, and checks that PostgreSQL refuses plaintext fallback. The containers and temporary certificates are removed on exit. CI runs this after the regular driver tests.

## Performance regression checks

Build the release app, then run the fixed desktop workload on a visible desktop:

```bash
cargo build --locked --release -p fastdb-app
cargo xtask benchmark --output benchmarks/results/baseline.json --display-id built-in-60hz-2x
# After a change and rebuild, compare on the same machine/display/toolchain:
cargo xtask benchmark --output benchmarks/results/candidate.json --display-id built-in-60hz-2x \
  --baseline benchmarks/results/baseline.json
```

The suite measures process startup, idle/peak RSS, streaming 1,000,000 rows x 20
columns through SQLite and the spilling pager, real grid scrolling p95/stutters,
and release executable size. It discards a warmup launch and checks median results
from five processes against absolute budgets and optional baseline regression
limits. Reports retain per-run metrics, frame intervals and build/host provenance.
See [benchmark protocol, thresholds and CI setup](../../benchmarks/README.md). The original
performance goals in [performance-goals.md](performance-goals.md) remain aspirational; these initial budgets are deliberately
broader. The manual desktop workflow requires a dedicated display host.

## Continuous integration

[`.github/workflows/ci.yml`](../../.github/workflows/ci.yml) runs three jobs:

- The engine and driver crates against `postgres:16` and `mysql:8.4` service containers, with `FASTDB_REQUIRE_DB_TESTS` set.
- A full workspace build and test on macOS.
- A full workspace clippy, build and test on Linux.
