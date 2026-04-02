#!/usr/bin/env bash

set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
OUT_DIR="${1:-$ROOT_DIR/.local/android-signing}"
KEYSTORE_NAME="${KEYSTORE_NAME:-openpup-release.jks}"
KEY_ALIAS="${KEY_ALIAS:-openpup}"
KEYSTORE_TYPE="${KEYSTORE_TYPE:-PKCS12}"
KEY_ALG="${KEY_ALG:-RSA}"
KEY_SIZE="${KEY_SIZE:-2048}"
VALIDITY_DAYS="${VALIDITY_DAYS:-10000}"
DNAME="${DNAME:-CN=OpenPup, OU=Mobile, O=OpenPup, L=Shanghai, ST=Shanghai, C=CN}"

rand_password() {
  local password
  password="$(LC_ALL=C dd if=/dev/urandom bs=32 count=1 2>/dev/null | base64 | tr -d '\n' | tr -dc 'A-Za-z0-9' | cut -c1-24)"
  if [[ -z "$password" ]]; then
    echo "Failed to generate random password" >&2
    exit 1
  fi
  printf '%s' "$password"
}

require_command() {
  if ! command -v "$1" >/dev/null 2>&1; then
    echo "Missing required command: $1" >&2
    exit 1
  fi
}

require_command keytool
require_command base64

mkdir -p "$OUT_DIR"

KEYSTORE_PATH="$OUT_DIR/$KEYSTORE_NAME"
SECRETS_ENV_PATH="$OUT_DIR/github-actions-secrets.env"
SECRETS_TXT_PATH="$OUT_DIR/github-actions-secrets.txt"

if [[ -e "$KEYSTORE_PATH" ]]; then
  echo "Refusing to overwrite existing keystore: $KEYSTORE_PATH" >&2
  echo "Delete it manually or pass a different output directory." >&2
  exit 1
fi

STORE_PASSWORD="${ANDROID_KEYSTORE_PASSWORD:-$(rand_password)}"
KEY_PASSWORD="${ANDROID_KEY_PASSWORD:-$STORE_PASSWORD}"

echo "Generating Android release keystore..."
echo "Output directory: $OUT_DIR"
echo "Keystore type: $KEYSTORE_TYPE"

keytool -genkeypair \
  -v \
  -keystore "$KEYSTORE_PATH" \
  -storetype "$KEYSTORE_TYPE" \
  -storepass "$STORE_PASSWORD" \
  -alias "$KEY_ALIAS" \
  -keypass "$KEY_PASSWORD" \
  -keyalg "$KEY_ALG" \
  -keysize "$KEY_SIZE" \
  -validity "$VALIDITY_DAYS" \
  -dname "$DNAME" \
  -noprompt

KEYSTORE_BASE64="$(base64 <"$KEYSTORE_PATH" | tr -d '\n')"

echo "Writing GitHub Actions secrets templates..."

cat >"$SECRETS_ENV_PATH" <<EOF
ANDROID_KEYSTORE_BASE64=$KEYSTORE_BASE64
ANDROID_KEYSTORE_PASSWORD=$STORE_PASSWORD
ANDROID_KEY_ALIAS=$KEY_ALIAS
ANDROID_KEY_PASSWORD=$KEY_PASSWORD
EOF

cat >"$SECRETS_TXT_PATH" <<EOF
GitHub Actions secrets for .github/workflows/android-test.yml

ANDROID_KEYSTORE_BASE64
$KEYSTORE_BASE64

ANDROID_KEYSTORE_PASSWORD
$STORE_PASSWORD

ANDROID_KEY_ALIAS
$KEY_ALIAS

ANDROID_KEY_PASSWORD
$KEY_PASSWORD
EOF

cat <<EOF
Done.

Keystore:
  $KEYSTORE_PATH

Signing parameters:
  store password: $STORE_PASSWORD
  key alias: $KEY_ALIAS
  key password: $KEY_PASSWORD

GitHub Actions secrets files:
  $SECRETS_ENV_PATH
  $SECRETS_TXT_PATH

Use these 4 secrets in GitHub Actions:
  ANDROID_KEYSTORE_BASE64
  ANDROID_KEYSTORE_PASSWORD
  ANDROID_KEY_ALIAS
  ANDROID_KEY_PASSWORD

Example:
  bash scripts/gen_android_release_keystore.sh
  bash scripts/gen_android_release_keystore.sh /absolute/output/dir
EOF
