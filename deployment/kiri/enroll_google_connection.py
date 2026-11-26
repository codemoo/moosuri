"""Run inside the enrolled member's Kiri VM. Only its public encryption key leaves."""
import base64
import json
import os
import sys
from uuid import UUID
from cryptography.hazmat.primitives import serialization
from cryptography.hazmat.primitives.asymmetric import rsa
import google_connection as connector

def main():
    os.umask(0o077)
    account,email,origin=sys.argv[1:]
    if str(UUID(account))!=account or not connector.google.EMAIL.fullmatch(email) or not origin.startswith('https://') or connector.urlsplit(origin).netloc!=origin[8:]:
        raise ValueError()
    profile={'account':account, 'email':email, 'origin':origin}
    base=connector.fresh.BASE
    with connector.fresh._lock(base/'google-enrollment.lock', create=True):
        if connector.CONFIG.exists() and json.loads(connector.fresh._read(connector.CONFIG))!=profile:
            raise ValueError("different enrollment")
        if not connector.KEY.exists():
            key=rsa.generate_private_key(public_exponent=65537,key_size=3072)
            with connector.KEY.open('xb') as out:
                out.write(key.private_bytes(serialization.Encoding.PEM,serialization.PrivateFormat.PKCS8,serialization.NoEncryption()))
                out.flush()
                os.fsync(out.fileno())
        key=serialization.load_pem_private_key(connector.fresh._read(connector.KEY),password=None)
        if not connector.CONFIG.exists():
            with connector.CONFIG.open('x') as out:
                json.dump(profile,out)
                out.flush()
                os.fsync(out.fileno())
        public=key.public_key().public_bytes(serialization.Encoding.DER,serialization.PublicFormat.SubjectPublicKeyInfo)
        print(json.dumps({'account':account,'email':email,'spki':base64.b64encode(public).decode()}))

if __name__=='__main__':
    main()
