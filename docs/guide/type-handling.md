# Type handling

- **PostgreSQL**: every column is decoded from the binary wire format. Covered types include integers, floats, `NUMERIC` (exact), `TEXT`, `UUID`, `JSON(B)`, `BYTEA`, dates and times (including `TIMETZ`, `INTERVAL` and ±infinity), `INET`, `CIDR`, `MACADDR`, `MONEY`, `OID`, `"char"`, enums, domains and N-dimensional arrays. A type FastDB does not recognize is shown as text or as a hex literal; it is never shown as `NULL`.
- **MySQL**: result columns are typed from the wire column type. **SQLite**: result columns are typed from the declared column type.
