use std::path::PathBuf;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DbKind {
    Postgres,
    MySql,
}

/// Automatically detect Unix Domain Socket on macOS and Linux when host is localhost / 127.0.0.1
pub fn detect_local_unix_socket(kind: DbKind, host: &str, port: u16) -> Option<PathBuf> {
    if host != "localhost" && host != "127.0.0.1" && host != "::1" && !host.is_empty() {
        return None;
    }

    let candidates: Vec<String> = match kind {
        DbKind::Postgres => vec![
            format!("/tmp/.s.PGSQL.{port}"),                // macOS (Homebrew / Postgres.app)
            format!("/var/run/postgresql/.s.PGSQL.{port}"), // Linux (Debian/Ubuntu)
            format!("/run/postgresql/.s.PGSQL.{port}"),     // Linux (Arch/Fedora)
        ],
        DbKind::MySql => vec![
            "/tmp/mysql.sock".to_string(),                  // macOS
            "/var/run/mysqld/mysqld.sock".to_string(),      // Linux (Ubuntu/Debian)
            "/var/lib/mysql/mysql.sock".to_string(),        // Linux (RHEL/Fedora)
        ],
    };

    candidates
        .into_iter()
        .map(PathBuf::from)
        .find(|path| path.exists())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_remote_host_skips_socket() {
        assert_eq!(detect_local_unix_socket(DbKind::Postgres, "db.example.com", 5432), None);
        assert_eq!(detect_local_unix_socket(DbKind::MySql, "192.168.1.100", 3306), None);
    }

    #[test]
    fn test_local_host_candidates_evaluation() {
        // Will check existence of candidate sockets; returns Option<PathBuf>
        let _ = detect_local_unix_socket(DbKind::Postgres, "localhost", 5432);
        let _ = detect_local_unix_socket(DbKind::MySql, "127.0.0.1", 3306);
    }
}
