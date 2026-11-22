#!/bin/bash
# Makes the certificate that releases are signed with, once.
#
# macOS remembers what an app was allowed (a keychain item, the camera, the
# microphone) by the app's signature. An ad-hoc signature is different in
# every build, so every update asked again. A certificate of our own, the
# same for every release, makes the answers last. It is self-signed: it
# proves nothing to Gatekeeper, only that two releases come from one place.
#
#     ./scripts/make-signing-cert.sh
#
# writes build/signing/, and says which two secrets to give the repository.
# Whoever holds the .p12 can sign as Tilde: keep it out of the repository.
set -euo pipefail
cd "$(dirname "$0")/.."

NAME="Tilde Release"
OUT=build/signing
[ ! -e "$OUT/tilde-signing.p12" ] || { echo "$OUT/tilde-signing.p12 exists already; remove it to make another" >&2; exit 1; }
mkdir -p "$OUT"
chmod 700 "$OUT"
# The system's openssl writes the kind of .p12 that `security import` reads.
SSL=/usr/bin/openssl

cat > "$OUT/cert.conf" <<CONF
[req]
distinguished_name = dn
x509_extensions = ext
prompt = no
[dn]
CN = $NAME
[ext]
basicConstraints = critical, CA:false
keyUsage = critical, digitalSignature
extendedKeyUsage = critical, codeSigning
CONF
PASSWORD=$($SSL rand -hex 16)
$SSL req -x509 -newkey rsa:2048 -nodes -days 7300 -config "$OUT/cert.conf" \
    -keyout "$OUT/key.pem" -out "$OUT/cert.pem" 2>/dev/null
$SSL pkcs12 -export -name "$NAME" -inkey "$OUT/key.pem" -in "$OUT/cert.pem" \
    -out "$OUT/tilde-signing.p12" -passout "pass:$PASSWORD"
rm "$OUT/key.pem" "$OUT/cert.conf"
base64 -i "$OUT/tilde-signing.p12" -o "$OUT/SIGNING_CERTIFICATE.txt"
printf '%s' "$PASSWORD" > "$OUT/SIGNING_PASSWORD.txt"
chmod 600 "$OUT"/*

cat <<DONE
made $OUT/tilde-signing.p12, valid for twenty years

Give the repository two secrets (Settings > Secrets and variables > Actions):
    SIGNING_CERTIFICATE   the contents of $OUT/SIGNING_CERTIFICATE.txt
    SIGNING_PASSWORD      the contents of $OUT/SIGNING_PASSWORD.txt
Then keep tilde-signing.p12 and its password somewhere safe and remove $OUT.
Losing them means one more round of questions after the next release.
DONE
