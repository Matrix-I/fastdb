#!/usr/bin/env bash
# Isolated PostgreSQL/MySQL TLS integration tests. Requires Docker and OpenSSL.
set -euo pipefail
cd "$(dirname "${BASH_SOURCE[0]}")/.."
TLS_DIR="$(mktemp -d)"
PG_NAME="fastdb-tls-pg-$$"
PLAIN_NAME="fastdb-plain-pg-$$"
MYSQL_NAME="fastdb-tls-mysql-$$"
cleanup() {
    docker rm -f "$PG_NAME" "$PLAIN_NAME" "$MYSQL_NAME" >/dev/null 2>&1 || true
    rm -rf "$TLS_DIR"
}
trap cleanup EXIT
openssl req -x509 -newkey rsa:2048 -nodes -keyout "$TLS_DIR/ca.key" -out "$TLS_DIR/ca.pem" -days 2 -subj '/CN=FastDB TLS test CA' >/dev/null 2>&1
openssl req -newkey rsa:2048 -nodes -keyout "$TLS_DIR/server.key" -out "$TLS_DIR/server.csr" -subj '/CN=localhost' >/dev/null 2>&1
cat > "$TLS_DIR/extensions.cnf" <<'EXT'
subjectAltName=DNS:localhost
extendedKeyUsage=serverAuth
basicConstraints=CA:FALSE
EXT
openssl x509 -req -in "$TLS_DIR/server.csr" -CA "$TLS_DIR/ca.pem" -CAkey "$TLS_DIR/ca.key" -CAcreateserial -out "$TLS_DIR/server.pem" -days 2 -extfile "$TLS_DIR/extensions.cnf" >/dev/null 2>&1
openssl req -newkey rsa:2048 -nodes -keyout "$TLS_DIR/client.key" -out "$TLS_DIR/client.csr" -subj '/CN=fastdb_tls_client' >/dev/null 2>&1
printf '%s\n' 'extendedKeyUsage=clientAuth' 'basicConstraints=CA:FALSE' > "$TLS_DIR/client.cnf"
openssl x509 -req -in "$TLS_DIR/client.csr" -CA "$TLS_DIR/ca.pem" -CAkey "$TLS_DIR/ca.key" -CAserial "$TLS_DIR/ca.srl" -out "$TLS_DIR/client.pem" -days 2 -extfile "$TLS_DIR/client.cnf" >/dev/null 2>&1
openssl pkcs12 -export -in "$TLS_DIR/client.pem" -inkey "$TLS_DIR/client.key" -out "$TLS_DIR/client.p12" -passout pass:fixture-archive >/dev/null 2>&1
openssl pkcs12 -export -nokeys -in "$TLS_DIR/client.pem" -out "$TLS_DIR/cert-only.p12" -passout pass:fixture-archive >/dev/null 2>&1
cat > "$TLS_DIR/pg_hba.conf" <<'HBA'
local all all trust
hostssl all fastdb_tls_client all cert
host all fastdb_tls_client all reject
host all all all trust
HBA
# Container users must be able to read the disposable test certificates.
chmod 755 "$TLS_DIR"
chmod 644 "$TLS_DIR/server.key"
docker run -d --name "$PG_NAME" -p 127.0.0.1::5432 -v "$TLS_DIR:/certs:ro" -e POSTGRES_HOST_AUTH_METHOD=trust postgres:16 bash -c '
    cp /certs/server.key /tmp/server.key
    chown postgres:postgres /tmp/server.key
    chmod 600 /tmp/server.key
    exec docker-entrypoint.sh postgres -c ssl=on -c ssl_cert_file=/certs/server.pem -c ssl_key_file=/tmp/server.key -c ssl_ca_file=/certs/ca.pem -c hba_file=/certs/pg_hba.conf
' >/dev/null
docker run -d --name "$PLAIN_NAME" -p 127.0.0.1::5432 -e POSTGRES_HOST_AUTH_METHOD=trust postgres:16 -c ssl=off >/dev/null
docker run -d --name "$MYSQL_NAME" -p 127.0.0.1::3306 -v "$TLS_DIR:/certs:ro" -e MYSQL_ROOT_PASSWORD=fastdb -e MYSQL_ROOT_HOST=% mysql:8.4 --ssl-ca=/certs/ca.pem --ssl-cert=/certs/server.pem --ssl-key=/certs/server.key --require-secure-transport=ON >/dev/null
ready=false
for _ in {1..90}; do
    if docker exec "$PG_NAME" pg_isready -U postgres >/dev/null 2>&1 &&
       docker exec "$PLAIN_NAME" pg_isready -U postgres >/dev/null 2>&1 &&
       docker exec "$MYSQL_NAME" mysql --protocol=TCP -h127.0.0.1 -uroot -pfastdb --ssl-mode=REQUIRED -e 'SELECT 1' >/dev/null 2>&1; then
        ready=true
        break
    fi
    sleep 2
done
if [ "$ready" != true ]; then
    docker logs "$PG_NAME"
    docker logs "$MYSQL_NAME"
    exit 1
fi
docker exec "$PG_NAME" psql -U postgres -v ON_ERROR_STOP=1 -c 'CREATE ROLE fastdb_tls_client LOGIN' >/dev/null
docker exec "$MYSQL_NAME" mysql -uroot -pfastdb -e "CREATE DATABASE fastdb_tls; CREATE USER 'fastdb_tls_client'@'%' IDENTIFIED BY 'fastdb' REQUIRE X509; GRANT SELECT ON fastdb_tls.* TO 'fastdb_tls_client'@'%'" >/dev/null
export FASTDB_TLS_CLIENT_ARCHIVE="$TLS_DIR/client.p12" FASTDB_TLS_CERT_ONLY_ARCHIVE="$TLS_DIR/cert-only.p12"
FASTDB_TLS_PG_PORT="$(docker port "$PG_NAME" 5432/tcp | head -1 | cut -d: -f2)"
FASTDB_TLS_PG_PLAIN_PORT="$(docker port "$PLAIN_NAME" 5432/tcp | head -1 | cut -d: -f2)"
FASTDB_TLS_MYSQL_PORT="$(docker port "$MYSQL_NAME" 3306/tcp | head -1 | cut -d: -f2)"
export FASTDB_TLS_PG_PORT FASTDB_TLS_PG_PLAIN_PORT FASTDB_TLS_MYSQL_PORT
export FASTDB_TLS_CA="$TLS_DIR/ca.pem"
FASTDB_REQUIRE_TLS_TESTS=1 cargo test -p db-wire --test tls_test -- --nocapture
FASTDB_REQUIRE_TLS_TESTS=1 cargo test -p db-wire --lib ssh_tls_ -- --nocapture
