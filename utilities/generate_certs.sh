#!/bin/bash
set -e

# Get the directory where the script is located
SCRIPT_DIR="$( cd "$( dirname "${BASH_SOURCE[0]}" )" &> /dev/null && pwd )"
CERTS_DIR="$SCRIPT_DIR/../certs"

mkdir -p "$CERTS_DIR"
echo "Generating certs in $CERTS_DIR"

# 1. Generate CA Key and Cert
openssl req -x509 -new -nodes -days 3650 -keyout "$CERTS_DIR/ca.key" -out "$CERTS_DIR/ca.crt" -subj "/CN=MyLocalCA"

# 2. Generate Server Key and CSR
openssl req -new -nodes -newkey rsa:2048 -keyout "$CERTS_DIR/server.key" -out "$CERTS_DIR/server.csr" -subj "/CN=localhost"

# 3. Sign Server CSR with CA
openssl x509 -req -in "$CERTS_DIR/server.csr" -CA "$CERTS_DIR/ca.crt" -CAkey "$CERTS_DIR/ca.key" -CAcreateserial -out "$CERTS_DIR/server.crt" -days 365

# 4. Create Chain (Server + CA)
cat "$CERTS_DIR/server.crt" "$CERTS_DIR/ca.crt" > "$CERTS_DIR/server.chain.crt"

echo "✅ Generated all certificates successfully:"
ls -l "$CERTS_DIR"
