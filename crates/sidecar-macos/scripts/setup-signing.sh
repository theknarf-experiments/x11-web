#!/usr/bin/env bash
#
# One-time setup: generate a self-signed code-signing certificate
# stored in the user's login keychain. After this, build-app.sh signs
# the .app bundle with the new identity instead of ad-hoc, and TCC
# grants for the bundle survive across rebuilds.
#
# Why we need this: macOS TCC pins a Screen Recording grant to the
# bundle's *designated requirement*. For ad-hoc-signed bundles that
# requirement is `cdhash H"<binary-hash>"` — every `cargo build`
# changes the hash, breaks the requirement, and forces the user to
# re-grant Screen Recording. With a stable signing identity the
# designated requirement becomes `identifier <bundle> and anchor leaf
# = certificate "<our-cert>"`, which any build signed by the same
# cert satisfies. Grant survives rebuilds.
#
# The cert is self-signed (no Apple Developer Account required) and
# lives only in the user's login keychain — it's not trusted by
# anyone but the local machine, which is exactly what we want for a
# dev-only signing identity.
#
# Idempotent: re-running detects the existing cert and exits.

set -euo pipefail

CERT_NAME="x11-web-sidecar-dev"
KEYCHAIN="$HOME/Library/Keychains/login.keychain-db"

if security find-certificate -c "$CERT_NAME" "$KEYCHAIN" >/dev/null 2>&1; then
    echo "Signing identity '$CERT_NAME' already present in login keychain."
    echo "Nothing to do."
    exit 0
fi

# OpenSSL needs a config file with the right extensions; -addext on
# the CLI works on newer OpenSSL but we go with a config for
# portability across macOS's bundled LibreSSL and Homebrew's OpenSSL.
TMPDIR_CERT=$(mktemp -d)
trap 'rm -rf "$TMPDIR_CERT"' EXIT

cat > "$TMPDIR_CERT/openssl.cnf" <<EOF
[req]
distinguished_name = dn
prompt             = no
[dn]
CN = $CERT_NAME
[v3_codesign]
keyUsage         = critical, digitalSignature
extendedKeyUsage = critical, codeSigning
basicConstraints = CA:FALSE
EOF

# 100-year cert (`-days 36500`). Local-only, no rotation pressure.
openssl req -x509 -nodes -newkey rsa:2048 \
    -keyout "$TMPDIR_CERT/key.pem" \
    -out "$TMPDIR_CERT/cert.pem" \
    -days 36500 \
    -config "$TMPDIR_CERT/openssl.cnf" \
    -extensions v3_codesign

# Bundle key + cert into PKCS#12 for `security import`. macOS's
# `security import` and modern OpenSSL disagree on how to handshake
# an empty PKCS#12 password (LibreSSL's MAC check rejects it), so we
# use a throwaway password and pass the same value to both ends.
P12_PASS=$(openssl rand -hex 16)
# `-legacy` selects the older PBE-SHA1-3DES + SHA1 MAC algorithm that
# macOS's `security` command can decrypt. OpenSSL 3.x defaults to
# AES-256-CBC + PBKDF2 + SHA-256 MAC, which `security` rejects with
# "MAC verification failed". `-legacy` is OpenSSL 3.x only — LibreSSL
# (which ships with macOS) uses the legacy algorithm by default, so
# the flag is rejected there. We probe the help output to decide.
P12_LEGACY_FLAG=()
if openssl pkcs12 -help 2>&1 | grep -q -- '-legacy'; then
    P12_LEGACY_FLAG=("-legacy")
fi
openssl pkcs12 -export \
    "${P12_LEGACY_FLAG[@]}" \
    -inkey "$TMPDIR_CERT/key.pem" \
    -in "$TMPDIR_CERT/cert.pem" \
    -out "$TMPDIR_CERT/cert.p12" \
    -name "$CERT_NAME" \
    -passout pass:"$P12_PASS"

# `-T /usr/bin/codesign` allows codesign to use the private key
# without an ACL prompt on every signing call.
security import "$TMPDIR_CERT/cert.p12" \
    -k "$KEYCHAIN" \
    -P "$P12_PASS" \
    -T /usr/bin/codesign

# Re-set the partition list so codesign can use the key without
# triggering the "allow / always allow / deny" Keychain dialog the
# first time. macOS's keychain ACL split-brain otherwise pops up.
security set-key-partition-list \
    -S apple-tool:,apple:,codesign: \
    -s -k "" "$KEYCHAIN" >/dev/null 2>&1 || true

echo
echo "Created self-signed code-signing identity: $CERT_NAME"
echo "Stored in: $KEYCHAIN"
echo
echo "Next steps:"
echo "  1. Run scripts/build-app.sh — the .app bundle will be signed"
echo "     with the new identity instead of ad-hoc."
echo "  2. In System Settings → Privacy & Security → Screen &"
echo "     System Audio Recording, remove any old X11WebSidecar"
echo "     entries (they reference the old ad-hoc signature) and"
echo "     re-add the freshly-signed bundle."
echo "  3. Subsequent rebuilds will preserve the TCC grant."
