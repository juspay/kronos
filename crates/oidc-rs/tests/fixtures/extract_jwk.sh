#!/usr/bin/env bash
set -euo pipefail
HERE="$(cd "$(dirname "$0")" && pwd)"
PRIV="$HERE/test_rsa_priv.pem"
python3 - <<PY
import base64, subprocess
out = subprocess.check_output(["openssl","rsa","-in","$PRIV","-noout","-modulus"]).decode()
n_hex = out.strip().split("=")[1]
n_bytes = bytes.fromhex(n_hex)
def b64u(b): return base64.urlsafe_b64encode(b).rstrip(b"=").decode()
open("$HERE/test_rsa_n.txt","w").write(b64u(n_bytes))
open("$HERE/test_rsa_e.txt","w").write("AQAB")
PY
