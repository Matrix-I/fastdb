# Performance goals

FastDB is aiming for these targets against a typical JVM client. They remain aspirational; the [regression protocol](../../benchmarks/README.md) and [local measurements](../../benchmarks/VALIDATION.md) track measured behavior and acceptance budgets separately.

| Metric | DBeaver (Java / JDBC) | FastDB target |
| :--- | :--- | :--- |
| Cold start | 4.5 s – 12 s | < 120 ms |
| Idle RAM | 600 MB – 1.2 GB | < 35 MB |
| 1M rows × 20 cols query | 2.5 GB+ (risk of OOM) | ~80 MB, bounded by disk spill |
| Time to first row | 300 ms – 2,000 ms | < 10 ms |
| Grid scrolling | 15 – 45 FPS (GC pauses) | 120 FPS |
| Package size | ~130 MB (bundled JRE) | ~12 MB |

What exists today that these targets build on: a paged result store with an LRU cache and LZ4 disk spill, a virtualized grid that only lays out visible rows, and drivers that stream rows into the store as they arrive.
