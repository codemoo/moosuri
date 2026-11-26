ARG GOOGLE_IMAGE
FROM ${GOOGLE_IMAGE}
USER root
RUN apt-get update && apt-get install -y --no-install-recommends python3 libnss3-tools && rm -rf /var/lib/apt/lists/* \
    && mkdir -p /capsule/test-tls \
    && openssl req -x509 -newkey rsa:2048 -nodes -days 3650 -subj '/CN=Moosuri Synthetic Google Fixture' -keyout /capsule/test-tls/ca.key -out /capsule/test-tls/ca.crt >/dev/null 2>&1 \
    && openssl req -newkey rsa:2048 -nodes -subj '/CN=accounts.google.com' -keyout /capsule/test-tls/server.key -out /capsule/test-tls/server.csr >/dev/null 2>&1 \
    && printf '%s\n' 'subjectAltName=DNS:accounts.google.com,DNS:myaccount.google.com,DNS:moosuri.example.test' 'basicConstraints=critical,CA:FALSE' 'keyUsage=critical,digitalSignature,keyEncipherment' 'extendedKeyUsage=serverAuth' > /capsule/test-tls/server.ext \
    && openssl x509 -req -in /capsule/test-tls/server.csr -CA /capsule/test-tls/ca.crt -CAkey /capsule/test-tls/ca.key -CAcreateserial -days 3650 -extfile /capsule/test-tls/server.ext -out /capsule/test-tls/server.crt >/dev/null 2>&1 \
    && rm /capsule/test-tls/ca.key /capsule/test-tls/server.csr /capsule/test-tls/server.ext /capsule/test-tls/ca.srl \
    && chmod 0400 /capsule/test-tls/server.key \
    && mv /usr/bin/google-chrome-stable /usr/bin/google-chrome-stable-real
COPY deployment/browser/google-chrome-test.sh /usr/bin/google-chrome-stable
COPY deployment/browser/google-fixture.py /capsule/google-fixture.py
RUN chmod 0555 /usr/bin/google-chrome-stable
