#!/usr/bin/env python
"""Makes the OpenPGP and S/MIME test messages in core/tests/fixtures/crypto/ with GnuPG and
OpenSSL, so the core is checked against what real programs write (not against itself).

Needs `gpg` (2.4) and `openssl` (3.x) on the PATH. The keys are throwaway and public: they only
exist to sign and encrypt these test files. Run from the repository root: python dev/make_crypto_fixtures.py
"""
import os
import re
import shutil
import subprocess
import sys
import tempfile

OUT = os.path.join("core", "tests", "fixtures", "crypto")
BOB_PASS = "bob-pass"
CAROL_PASS = "carol-pass"
HOME = tempfile.mkdtemp(prefix="kweb-gnupg-")
os.chmod(HOME, 0o700)
# GnuPG from Git for Windows wants a POSIX path.
GNUPGHOME = subprocess.check_output(["cygpath", "-u", HOME]).decode().strip() if shutil.which("cygpath") else HOME
ENV = dict(os.environ, GNUPGHOME=GNUPGHOME)


def run(args, data=None, env=None, check=True):
    p = subprocess.run(args, input=data, capture_output=True, env=env or ENV)
    if check and p.returncode != 0:
        sys.exit(f"failed: {' '.join(args)}\n{p.stderr.decode(errors='replace')}")
    return p.stdout


def gpg(*args, data=None):
    return run(["gpg", "--batch", "--yes", "--pinentry-mode", "loopback", "--no-tty", *args], data)


def write(name, data):
    mode = "wb" if isinstance(data, bytes) else "w"
    with open(os.path.join(OUT, name), mode, **({} if mode == "wb" else {"newline": ""})) as f:
        f.write(data)


def crlf(text):
    return re.sub(r"\r*\n", "\r\n", text)


def headers(frm, to, subject, extra=""):
    return crlf(
        f"From: {frm}\nTo: {to}\nSubject: {subject}\nDate: Tue, 11 Nov 2025 10:00:00 +0000\n"
        f"Message-ID: <{abs(hash(subject)) % 10**9}@example.test>\nMIME-Version: 1.0\n{extra}"
    )


# ---- OpenPGP keys -----------------------------------------------------------------------------
def make_key(name, address, passphrase):
    uid = f"{name} <{address}>"
    gpg("--passphrase", passphrase, "--quick-gen-key", uid, "default", "default", "never")
    pub = gpg("--armor", "--export", address)
    sec = gpg("--armor", "--passphrase", passphrase, "--export-secret-keys", address)
    fpr = [l for l in gpg("--with-colons", "--fingerprint", address).decode().splitlines() if l.startswith("fpr")][0].split(":")[9]
    write(f"{name.lower()}.pub.asc", pub)
    write(f"{name.lower()}.sec.asc", sec)
    return fpr


def main():
    os.makedirs(OUT, exist_ok=True)
    fprs = {}
    fprs["alice"] = make_key("Alice", "alice@example.test", "")
    fprs["bob"] = make_key("Bob", "bob@example.test", BOB_PASS)
    fprs["mallory"] = make_key("Mallory", "mallory@example.test", "")
    write("fingerprints.txt", "".join(f"{k} {v}\n" for k, v in fprs.items()))

    # --- multipart/signed (PGP/MIME), signed by Alice
    part = crlf("Content-Type: text/plain; charset=utf-8\nContent-Transfer-Encoding: 7bit\n\nHello Bob, this is signed.\n")
    part_bytes = part.rstrip("\r\n").encode()  # the part ends before the line ending of the delimiter
    sig = gpg("--armor", "--detach-sign", "-u", "alice@example.test", "--passphrase", "", data=part_bytes).decode()
    b = "pgp-boundary-1"
    signed = (
        headers("Alice <alice@example.test>", "Bob <bob@example.test>", "Signed",
                f'Content-Type: multipart/signed; micalg=pgp-sha256; protocol="application/pgp-signature"; boundary="{b}"\n')
        + crlf(f"\n--{b}\n") + part.rstrip("\r\n") + crlf(f"\n--{b}\nContent-Type: application/pgp-signature; name=\"signature.asc\"\n\n")
        + crlf(sig) + crlf(f"--{b}--\n")
    )
    write("pgp-signed.eml", signed)

    # --- clear-signed inline
    clear = gpg("--clear-sign", "-u", "alice@example.test", "--passphrase", "", data=b"Hello Bob, this is clear-signed.\n").decode()
    write("pgp-clearsigned.eml", headers("Alice <alice@example.test>", "Bob <bob@example.test>", "Clear",
                                         "Content-Type: text/plain; charset=utf-8\nContent-Transfer-Encoding: 7bit\n") + crlf("\n" + clear))

    # --- PGP/MIME encrypted to Bob, signed by Alice inside
    inner = crlf("Content-Type: text/plain; charset=utf-8\n\nSecret hello, signed and sealed.\n")
    for label, signing in (("signed-", ["-u", "alice@example.test", "--sign"]), ("", [])):
        enc = gpg("--armor", "--trust-model", "always", "--encrypt", "-r", "bob@example.test", *signing,
                  "--passphrase", "", data=inner.encode()).decode()
        b = "pgp-enc-boundary"
        msg = (
            headers("Alice <alice@example.test>", "Bob <bob@example.test>", "Encrypted",
                    f'Content-Type: multipart/encrypted; protocol="application/pgp-encrypted"; boundary="{b}"\n')
            + crlf(f"\n--{b}\nContent-Type: application/pgp-encrypted\n\nVersion: 1\n\n--{b}\nContent-Type: application/octet-stream; name=\"encrypted.asc\"\n\n")
            + crlf(enc) + crlf(f"--{b}--\n")
        )
        write(f"pgp-encrypted-{label}bob.eml".replace("--", "-"), msg)

    # --- inline encrypted
    enc = gpg("--armor", "--trust-model", "always", "--encrypt", "-r", "bob@example.test", data=b"Inline secret.\n").decode()
    write("pgp-inline-encrypted.eml", headers("Alice <alice@example.test>", "Bob <bob@example.test>", "Inline",
                                              "Content-Type: text/plain; charset=utf-8\n") + crlf("\n" + enc))

    # --- a key that was revoked after it signed something
    fprs["rex"] = make_key("Rex", "rex@example.test", "")
    rex_part = crlf("Content-Type: text/plain; charset=utf-8\n\nSigned by a key that is revoked later.\n").rstrip("\r\n")
    rex_sig = gpg("--armor", "--detach-sign", "-u", "rex@example.test", "--passphrase", "", data=rex_part.encode()).decode()
    b = "rex-boundary"
    write("pgp-signed-rex.eml", headers("Rex <rex@example.test>", "Bob <bob@example.test>", "Revoked",
          f'Content-Type: multipart/signed; micalg=pgp-sha256; protocol="application/pgp-signature"; boundary="{b}"\n')
          + crlf(f"\n--{b}\n") + rex_part + crlf(f"\n--{b}\nContent-Type: application/pgp-signature\n\n") + crlf(rex_sig) + crlf(f"--{b}--\n"))
    rev_path = os.path.join(HOME, "openpgp-revocs.d", fprs["rex"] + ".rev")
    rev = re.sub(r"(?m)^:(-----)", r"\1", open(rev_path).read())
    gpg("--import", data=rev.encode())
    write("rex-revoked.pub.asc", gpg("--armor", "--export", "rex@example.test"))
    write("fingerprints.txt", "".join(f"{k} {v}\n" for k, v in fprs.items()))

    # --- a public key attached to a message
    write("pgp-keys-attached.eml", headers("Alice <alice@example.test>", "Bob <bob@example.test>", "My key",
          'Content-Type: multipart/mixed; boundary="kk"\n') + crlf("\n--kk\nContent-Type: text/plain\n\nMy key is attached.\n\n--kk\n"
          "Content-Type: application/pgp-keys; name=\"alice.asc\"\nContent-Disposition: attachment; filename=\"alice.asc\"\n\n")
          + crlf(open(os.path.join(OUT, "alice.pub.asc"), newline="").read()) + crlf("--kk--\n"))

    # ---- S/MIME with OpenSSL ----------------------------------------------------------------------
    ssl_home = tempfile.mkdtemp(prefix="kweb-ssl-")
    def ssl(*args, data=None, check=True):
        return run(["openssl", *args], data=data, env=dict(os.environ), check=check)

    def make_cert(name, cn, address, key_args, days=7300, not_before=None, not_after=None):
        key = os.path.join(ssl_home, f"{name}.key")
        crt = os.path.join(ssl_home, f"{name}.crt")
        ssl("genpkey", *key_args, "-out", key)
        cfg = os.path.join(ssl_home, f"{name}.cnf")
        with open(cfg, "w") as f:
            f.write(f"[req]\ndistinguished_name=dn\nx509_extensions=ext\nprompt=no\n[dn]\nCN={cn}\nemailAddress={address}\n"
                    f"[ext]\nsubjectAltName=email:{address}\nkeyUsage=digitalSignature,keyEncipherment\n"
                    f"extendedKeyUsage=emailProtection\nsubjectKeyIdentifier=hash\n")
        extra = []
        if not_before:
            extra += ["-not_before", not_before, "-not_after", not_after]
        else:
            extra += ["-days", str(days)]
        ssl("req", "-new", "-x509", "-key", key, "-out", crt, "-config", cfg, "-sha256", *extra)
        return key, crt

    rsa = ["-algorithm", "RSA", "-pkeyopt", "rsa_keygen_bits:2048"]
    carol_key, carol_crt = make_cert("carol", "Carol Example", "carol@example.test", rsa)
    erin_key, erin_crt = make_cert("erin", "Erin Example", "erin@example.test", ["-algorithm", "EC", "-pkeyopt", "ec_paramgen_curve:P-256"])
    old_key, old_crt = make_cert("olga", "Olga Example", "olga@example.test", rsa, not_before="20200101000000Z", not_after="20200301000000Z")
    for n in ("carol", "erin", "olga"):
        shutil.copy(os.path.join(ssl_home, f"{n}.crt"), os.path.join(OUT, f"{n}.crt.pem"))
    shutil.copy(carol_key, os.path.join(OUT, "carol.key.pem"))

    ssl("pkcs12", "-export", "-inkey", carol_key, "-in", carol_crt, "-name", "Carol", "-passout", f"pass:{CAROL_PASS}",
        "-out", os.path.join(OUT, "carol.p12"))
    ssl("pkcs12", "-export", "-legacy", "-inkey", carol_key, "-in", carol_crt, "-name", "Carol", "-passout", f"pass:{CAROL_PASS}",
        "-out", os.path.join(OUT, "carol-legacy.p12"), check=False)

    body = crlf("Content-Type: text/plain; charset=utf-8\nContent-Transfer-Encoding: 7bit\n\nHello, this is S/MIME signed.\n")
    inpath = os.path.join(ssl_home, "in.txt")
    with open(inpath, "wb") as f:
        f.write(body.encode())

    def smime(frm, subject, *args, signer=None):
        out = ssl("smime", "-in", inpath, *args).decode(errors="replace")
        out = re.sub(r"MIME-Version: 1.0\r\n", "", crlf(out), count=1)
        return headers(frm, "Bob <bob@example.test>", subject) + out

    write("smime-signed.eml", smime("Carol Example <carol@example.test>", "SMIME signed", "-sign", "-signer", carol_crt, "-inkey", carol_key, "-md", "sha256"))
    write("smime-signed-ec.eml", smime("Erin Example <erin@example.test>", "SMIME signed EC", "-sign", "-signer", erin_crt, "-inkey", erin_key, "-md", "sha256"))
    write("smime-signed-old.eml", smime("Olga Example <olga@example.test>", "SMIME signed old", "-sign", "-signer", old_crt, "-inkey", old_key, "-md", "sha256"))
    write("smime-signed-sha1.eml", smime("Carol Example <carol@example.test>", "SMIME sha1", "-sign", "-signer", carol_crt, "-inkey", carol_key, "-md", "sha1"))
    write("smime-opaque.eml", smime("Carol Example <carol@example.test>", "SMIME opaque", "-sign", "-nodetach", "-signer", carol_crt, "-inkey", carol_key, "-md", "sha256"))
    write("smime-encrypted.eml", smime("Erin Example <erin@example.test>", "SMIME encrypted", "-encrypt", "-aes256", carol_crt))
    write("smime-encrypted-aes128.eml", smime("Erin Example <erin@example.test>", "SMIME encrypted 128", "-encrypt", "-aes128", carol_crt))
    # signed, then encrypted (what mail programs call "sign and encrypt")
    signed_inner = ssl("smime", "-in", inpath, "-sign", "-signer", erin_crt, "-inkey", erin_key, "-md", "sha256").decode(errors="replace")
    with open(inpath, "wb") as f:
        f.write(crlf(signed_inner).encode())
    write("smime-signed-encrypted.eml", smime("Erin Example <erin@example.test>", "SMIME signed then encrypted", "-encrypt", "-aes256", carol_crt))
    # authenticated enveloped data (newer): AES-GCM
    with open(inpath, "wb") as f:
        f.write(body.encode())
    out = ssl("cms", "-encrypt", "-aes-256-gcm", "-in", inpath, "-recip", carol_crt, "-outform", "SMIME", check=False).decode(errors="replace")
    if out:
        write("smime-encrypted-gcm.eml", headers("Erin Example <erin@example.test>", "Bob <bob@example.test>", "SMIME gcm") + crlf(out))

    shutil.rmtree(ssl_home, ignore_errors=True)
    run(["gpgconf", "--kill", "gpg-agent"], check=False)
    shutil.rmtree(HOME, ignore_errors=True)
    print("fixtures written to", OUT)


if __name__ == "__main__":
    main()
