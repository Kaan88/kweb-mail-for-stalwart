#!/usr/bin/env python
"""Has GnuPG and OpenSSL read the messages the core writes (stage 3 of M9), so they are checked against other
programs and not only against this one's own reader.

Needs `gpg` (2.4) and `openssl` (3.x) on the PATH. Run from the repository root: python dev/check_crypto_built.py
The keys are the throwaway ones of core/tests/fixtures/crypto (see make_crypto_fixtures.py).
"""
import os
import re
import shutil
import subprocess
import sys
import tempfile

FIX = os.path.join("core", "tests", "fixtures", "crypto")
OUT = tempfile.mkdtemp(prefix="kweb-built-")
HOME = tempfile.mkdtemp(prefix="kweb-gnupg-")
os.chmod(HOME, 0o700)
GNUPGHOME = subprocess.check_output(["cygpath", "-u", HOME]).decode().strip() if shutil.which("cygpath") else HOME
ENV = dict(os.environ, GNUPGHOME=GNUPGHOME)
failures = []


def run(args, data=None, env=None):
    p = subprocess.run(args, input=data, capture_output=True, env=env or ENV)
    return p.returncode, p.stdout, p.stderr.decode(errors="replace")


def gpg(*args, data=None):
    return run(["gpg", "--batch", "--yes", "--pinentry-mode", "loopback", "--no-tty", *args], data)


def check(name, ok, detail=""):
    print(("ok    " if ok else "FAIL  ") + name)
    if not ok:
        failures.append(name)
        if detail:
            print("      " + detail.strip().replace("\n", "\n      "))


def read(name):
    with open(os.path.join(OUT, name), "rb") as f:
        return f.read()


def mime_parts(raw):
    """The header block and the parts of a multipart message, as the exact bytes between the boundaries."""
    head, _, body = raw.partition(b"\r\n\r\n")
    boundary = re.search(rb'boundary="([^"]+)"', head).group(1)
    pieces = body.split(b"--" + boundary)
    parts = []
    for piece in pieces[1:]:
        if piece.startswith(b"--"):
            break
        # between "--b\r\n" and "\r\n" before the next delimiter
        parts.append(piece[2:-2] if piece.endswith(b"\r\n") else piece[2:])
    return head, parts


def main():
    env = dict(os.environ, KWEB_CRYPTO_OUT=OUT)
    code = subprocess.call(["cargo", "test", "-p", "kweb-core", "--lib", "messages_for_other_programs"], env=env)
    if code != 0:
        sys.exit("the core could not write the messages")
    for key in ("alice.pub.asc", "bob.pub.asc", "bob.sec.asc"):
        rc, _, err = gpg("--passphrase", "bob-pass", "--import", os.path.join(FIX, key))
        if rc != 0:
            sys.exit("gpg import failed: " + err)

    # OpenPGP signed
    raw = read("pgp-signed.eml")
    _, parts = mime_parts(raw)
    signed, signature = parts[0], parts[1].split(b"\r\n\r\n", 1)[1]
    with open(os.path.join(OUT, "signed.bin"), "wb") as f:
        f.write(signed)
    with open(os.path.join(OUT, "signed.asc"), "wb") as f:
        f.write(signature)
    rc, _, err = gpg("--verify", os.path.join(OUT, "signed.asc"), os.path.join(OUT, "signed.bin"))
    check("GnuPG verifies the PGP/MIME signature", rc == 0 and "Good signature" in err, err)

    # OpenPGP signed and encrypted
    raw = read("pgp-encrypted.eml")
    _, parts = mime_parts(raw)
    armored = parts[1].split(b"\r\n\r\n", 1)[1]
    rc, out, err = gpg("--passphrase", "bob-pass", "--decrypt", data=armored)
    check("GnuPG decrypts the PGP/MIME message for Bob", rc == 0 and b"Secret plans" in out, err)
    check("GnuPG finds the signature inside it", "Good signature" in err and "alice@example.test" in err, err)
    check("the attachment is inside the encryption", b"plan.txt" in out)

    # S/MIME
    cert = os.path.join(FIX, "carol.crt.pem")
    key = os.path.join(FIX, "carol.key.pem")
    path = os.path.join(OUT, "smime-signed.eml")
    rc, out, err = run(["openssl", "smime", "-verify", "-in", path, "-certfile", cert, "-noverify"], env=os.environ)
    check("OpenSSL verifies the S/MIME signature", rc == 0 and b"Secret plans" in out, err)
    path = os.path.join(OUT, "smime-encrypted.eml")
    rc, out, err = run(["openssl", "smime", "-decrypt", "-in", path, "-recip", cert, "-inkey", key], env=os.environ)
    check("OpenSSL decrypts the S/MIME message", rc == 0 and b"multipart/signed" in out, err)
    with open(os.path.join(OUT, "inner.eml"), "wb") as f:
        f.write(out)
    rc, out2, err = run(["openssl", "smime", "-verify", "-in", os.path.join(OUT, "inner.eml"), "-certfile", cert, "-noverify"], env=os.environ)
    check("and verifies the signature inside it", rc == 0 and b"Secret plans" in out2 and b"plan.txt" in out2, err)

    shutil.rmtree(OUT, ignore_errors=True)
    shutil.rmtree(HOME, ignore_errors=True)
    if failures:
        sys.exit(f"{len(failures)} check(s) failed")
    print("all checks passed")


main()
