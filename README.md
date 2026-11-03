# FastDB ⚡

> **A native Rust database client — aiming to replace JVM/JDBC tools like DBeaver**
> *No JVM · No JDBC · GPU-rendered UI (GPUI) · Bounded memory for huge result sets*

[![CI](https://github.com/Matrix-I/fastdb/actions/workflows/ci.yml/badge.svg)](https://github.com/Matrix-I/fastdb/actions/workflows/ci.yml)
[![Rust](https://img.shields.io/badge/Rust-stable-orange.svg?logo=rust&logoColor=white)](https://www.rust-lang.org)
[![UI](https://img.shields.io/badge/UI-GPUI-blue.svg)](https://github.com/zed-industries/zed)
[![Platform](https://img.shields.io/badge/Platform-macOS%20%7C%20Linux-lightgrey.svg)](ci/)

FastDB is a database client built from scratch in Rust. It talks to databases through native async drivers and vendor ODBC clients, renders its UI on the GPU with [GPUI](https://github.com/zed-industries/zed) (Metal on macOS, Vulkan on Linux), and keeps query results in a paged, disk-spilling memory engine so a million-row result does not need a million-row heap.

> **Status: early prototype.** Browsing tables, streaming their data and running free-form SQL from script tabs works against PostgreSQL, MySQL and SQLite. Several other features listed under [Not built yet](docs/guide/not-built-yet.md) exist only as tested library code or not at all. The performance numbers in [Performance goals](docs/development/performance-goals.md) are **targets, not measurements**.

---

## ⚡ Highlights

- **Providers**: PostgreSQL, MySQL / MariaDB and SQLite through native drivers; Db2 for LUW, Oracle and SQL Server through vendor ODBC drivers. TLS for PostgreSQL and MySQL, OS keychain for saved passwords.
- **Streaming results** into a paged store with an LRU cache and LZ4 disk spill, shown in a virtualized grid with inline editing, copy and CSV / SQL export.
- **DBeaver-style navigator** with catalog objects, search, ER diagrams and PostgreSQL Properties, DDL and column editor.
- **SQL editor** with per-connection script files, schema-aware completion, result tabs, persistent PostgreSQL sessions with transactions, and a visual query plan.
- **In-app updates** from GitHub Releases.

See the [guide](docs/guide/) for the full behavior of each area.

---

## 🚀 Quick Start

Download the Apple Silicon DMG or ZIP from [GitHub Releases](https://github.com/Matrix-I/fastdb/releases/latest) and follow the [installation guide](docs/guide/installation.md). To build from source:

```bash
git clone https://github.com/Matrix-I/fastdb.git
cd fastdb
cargo run --release -p fastdb-app
```

See [Building and packaging](docs/development/building.md) for prerequisites on macOS and Linux.

---

## 📚 Documentation

| Topic | Description |
| :--- | :--- |
| [Installation](docs/guide/installation.md) | macOS preview install and the Gatekeeper note |
| [Connections](docs/guide/connections.md) | Providers, TLS, keychain, where settings are stored |
| [Navigator and table tabs](docs/guide/navigator-and-tables.md) | Catalog browsing, search, data grid, Properties, ER diagram, column editor |
| [SQL editor](docs/guide/sql-editor.md) | Scripts, execution, transactions, query plans, history, formatting |
| [Type handling](docs/guide/type-handling.md) | How column types are decoded per provider |
| [Keyboard shortcuts](docs/guide/shortcuts.md) | macOS and Linux shortcut matrix |
| [Software updates](docs/guide/updates.md) | Update checks, release notes and installer download |
| [Not built yet](docs/guide/not-built-yet.md) | Known gaps and unsupported workflows |
| [Provider setup and live tests](docs/PROVIDERS.md) | Vendor ODBC drivers and provider containers |
| [Architecture](docs/development/architecture.md) | Streaming pipeline, workspace crates, memory engine |
| [Performance goals](docs/development/performance-goals.md) | Target numbers against a typical JVM client |
| [Building and packaging](docs/development/building.md) | Build from source, macOS bundle, signing, Linux packages |
| [Testing](docs/development/testing.md) | Unit and live tests, TLS tests, benchmarks, CI |
| [Milestones](docs/development/milestones.md) | Checklist of delivered and open milestones |
| [Roadmap](docs/ROADMAP.md) | Audited roadmap and source audit |
| [DBeaver workflows](docs/DBEAVER_WORKFLOWS.md) · [Workflow acceptance](docs/WORKFLOW_ACCEPTANCE.md) | GUI analysis and manual acceptance checklist |
| [Release plan](docs/RELEASE_PLAN.md) | Maintainer release checklist |
| [Plans](docs/plans/) | Per-feature implementation plans and historical records |

---

## 📄 License

MIT. See [LICENSE](LICENSE).
