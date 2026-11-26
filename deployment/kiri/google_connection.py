"""Private Kiri guest process for one vault-backed Google connection over stdin/stdout."""
import base64
import json
import os
import select
import sys
import time
from uuid import UUID
from urllib.parse import urlsplit, parse_qs

from cryptography.hazmat.primitives import hashes, serialization
from cryptography.hazmat.primitives.asymmetric import padding
from cryptography.hazmat.primitives.ciphers.aead import AESGCM
import fresh_browser as fresh
import secure_google as google

CONFIG = fresh.BASE / 'moosuri-google-connection.json'
KEY = fresh.BASE / 'moosuri-google-connection.pem'
PROTOCOL = 'moosuri-google-connection-v3'
class TeardownFailed(Exception):
    """Chrome may still be live, so no teardown acknowledgement may be sent."""


def connected_clock():
    if select.select([sys.stdin.fileno()], [], [], 0)[0]:
        raise ValueError("connection closed")
    return time.monotonic()


SCOPES = {'gmail':'gmail.readonly', 'drive':'drive.metadata.readonly', 'docs':'documents.readonly',
          'sheets':'spreadsheets.readonly', 'slides':'presentations.readonly'}


def binding(job, account, email, selected=None):
    return ('moosuri-google-v2\n' + job + '\n' + account + '\n' + email + '\n' + (selected or email)).encode()


def decode(value, limit):
    if not isinstance(value, str) or len(value) > limit:
        raise ValueError()
    return base64.b64decode(value, validate=True)


def wipe(value):
    if isinstance(value, bytearray):
        value[:] = b'\0' * len(value)


def validate_secret(value):
    try:
        if not value or len(value) > 4096 or 0 in value:
            raise ValueError()
        value.decode()
        return value
    except Exception:
        wipe(value)
        raise


def decrypt(envelope, key, label):
    if not isinstance(envelope, dict) or set(envelope) != {'key', 'iv', 'body'}:
        raise ValueError()
    symmetric = None
    value = None
    cipher = None
    try:
        symmetric = bytearray(key.decrypt(decode(envelope['key'], 768), padding.OAEP(
            mgf=padding.MGF1(hashes.SHA256()), algorithm=hashes.SHA256(), label=label)))
        iv = decode(envelope['iv'], 16)
        if len(iv) != 12 or len(symmetric) != 32:
            raise ValueError()
        cipher = AESGCM(symmetric)
        value = bytearray(cipher.decrypt(iv, decode(envelope['body'], 8200), label))
        return validate_secret(value)
    except Exception:
        wipe(value)
        raise
    finally:
        # Best-effort process-local cleanup: cryptography owns its internal key
        # schedule and returns an immutable plaintext bytes object before the
        # bytearray copy. Wipe the mutable key source and release the cipher;
        # the isolated one-shot guest process is the hard lifetime boundary.
        cipher = None
        wipe(symmetric)


def next_button(controls, labels):
    buttons = [c for c in controls if c.get('role') == 'button' and google.label(c) in labels]
    if len(buttons) == 1 and google.REF.fullmatch(buttons[0].get('ref', '')):
        return buttons[0]['ref']


def accounts_page(browser, tab, *, allow_verification=False):
    view = browser.call('controls', tab=tab)
    google.provider_stop(view)
    if view.get('reason') == 'verification':
        if allow_verification:
            return view
        raise google.Outcome('needs_human', 'verification')
    if view.get('origin') not in (None, google.ACCOUNTS, google.ACCOUNT_HOME):
        raise google.Outcome('blocked', 'unexpected_origin')
    return view


def login(browser, tab, email, secret, *, identified=False, clock=time.monotonic, sleep=time.sleep, on_verification=None):
    submitted = False
    awaiting_verification = False
    end = clock() + 90
    while clock() < end:
        view = accounts_page(browser, tab, allow_verification=True)
        controls = view.get('controls', [])
        challenge = view.get('reason') == 'verification'
        if (identified and view.get('origin') == google.ACCOUNTS and not google.identifier_controls(controls)
                and not any(c.get('type') == 'password' for c in controls)):
            page = browser.call('read', tab=tab)
            challenge = challenge or any(phrase in page.get('text', '').casefold() for phrase in google.PHONE)
        if challenge:
            if not identified:
                raise google.Outcome('needs_human', 'verification')
            if not awaiting_verification:
                awaiting_verification = True
                end = clock() + 120
                if on_verification:
                    on_verification()
            # The member approves on their own Google device. No challenge
            # contents, OTPs or cookies cross the gateway; polling is cancellable.
            sleep(.65)
            continue
        if view.get('origin') == google.ACCOUNT_HOME:
            if identified and google.account_matches(controls, email):
                return
        elif view.get('origin') == google.ACCOUNTS:
            pair = google.identifier_controls(controls)
            if pair and not identified:
                google.submit_identifier(browser, tab, pair, email)
                identified = True
            elif identified and not submitted:
                fields = [c for c in controls if c.get('type') == 'password' and c.get('editable') and c.get('credential')]
                button = next_button(controls, google.NEXT)
                if len(fields) == 1 and button and google.REF.fullmatch(fields[0].get('ref', '')):
                    # Fresh isolated profile and a single identifier submission. Also require
                    # Google's visible account chooser to identify this exact password recipient.
                    observed = {m.casefold() for c in controls for m in google.EMAIL.findall(c.get('label', ''))}
                    if not observed:
                        sleep(.65)
                        continue
                    if observed != {email.casefold()}:
                        raise google.Outcome('needs_human', 'account_unconfirmed')
                    acted = browser.call('fill', tab=tab, ref=fields[0]['ref'], origin=google.ACCOUNTS, text=secret.decode())
                    if acted.get('acted') is not True or browser.call('click', tab=tab, ref=button).get('acted') is not True:
                        raise google.Outcome('needs_human', 'form_changed')
                    submitted = True
        sleep(.65)
    raise google.Outcome('needs_human', 'verification')


def consent(browser, url, config, service, *, clock=time.monotonic, sleep=time.sleep):
    parts = urlsplit(url)
    fields = parse_qs(parts.query)
    if (parts.scheme, parts.netloc, parts.path, parts.fragment) != ('https', 'accounts.google.com', '/o/oauth2/v2/auth', '') or (
        fields.get('redirect_uri') != [config['origin'] + '/api/v1/mail/oauth/callback'] or
        fields.get('code_challenge_method') != ['S256'] or fields.get('login_hint') != [config['email']]):
        raise ValueError()
    expected = {'https://www.googleapis.com/auth/' + SCOPES[service]}
    if service != 'gmail':
        expected.update({'openid', 'https://www.googleapis.com/auth/userinfo.email'})
    if (len(fields.get('scope', [])) != 1 or set(fields['scope'][0].split()) != expected or
            fields.get('include_granted_scopes') != ['false'] or fields.get('response_type') != ['code']):
        raise ValueError()
    tab = browser.call('open', url=url).get('tab', '')
    if not google.TAB.fullmatch(tab):
        raise ValueError()
    end = clock() + 100
    clicked = set()
    while clock() < end:
        view = browser.call('controls', tab=tab)
        google.provider_stop(view)
        if view.get('origin') == config['origin']:
            return
        if view.get('reason') == 'verification' or view.get('origin') not in (None, google.ACCOUNTS):
            raise google.Outcome('needs_human', 'verification')
        controls = view.get('controls', [])
        accounts = [c for c in controls if c.get('role') in {'button','link'} and
                    config['email'].casefold() in {m.casefold() for m in google.EMAIL.findall(c.get('label', ''))}]
        button = next_button(controls, {'continue', 'allow', '계속', '허용'})
        ref = button or (accounts[0]['ref'] if len(accounts) == 1 else None)
        if ref and ref not in clicked:
            if not google.REF.fullmatch(ref) or len(clicked) >= 4 or browser.call('click', tab=tab, ref=ref).get('acted') is not True:
                raise google.Outcome('needs_human', 'form_changed')
            clicked.add(ref)
        sleep(.65)
    raise google.Outcome('needs_human', 'verification')


def main():
    config = json.loads(fresh._read(CONFIG))
    request = google.receive(2048, 12)
    job = request['job']
    if (set(request) != {'protocol', 'job', 'account', 'email', 'google_email', 'service'} or
        request['protocol'] != PROTOCOL or str(UUID(job)) != job or request['account'] != config['account'] or
        request['email'] != config['email'] or request['service'] not in SCOPES):
        raise ValueError()
    selected = request['google_email']
    if not isinstance(selected, str) or len(selected.encode()) > 254 or not google.EMAIL.fullmatch(selected):
        raise ValueError()
    key = serialization.load_pem_private_key(fresh._read(KEY), password=None)
    # A syntactically valid Deliver request is not enough to open Chrome. Kiri
    # first proves that the packet was sealed for this exact job/account/email.
    google.send({'status':'vault', 'protocol':PROTOCOL})
    packet = google.receive(12500, 600)
    secret = None
    browser = None
    returned = False
    try:
        # Unwrap once before Chrome exists. The gateway reauthorizes the member
        # after verification and must explicitly release Kiri before Chrome starts.
        # AESGCM and the CDP JSON call necessarily create short-lived immutable
        # Python copies that cannot be zeroized in place. Mutable copies are wiped,
        # and the isolated one-shot guest process is discarded at the boundary.
        secret = decrypt(packet, key, binding(job, config['account'], config['email'], selected))
        config = dict(config, email=selected)
        google.send({'status':'verified'})
        release = google.receive(256, 45)
        if release != {'status':'proceed'}:
            raise ValueError()
        browser = fresh.FreshBrowser.create()
        # Browser creation has its own bounded container startup window. If the
        # member session was revoked during it, discard without opening a tab.
        connected_clock()
        tab = browser.call('open', url=google.START).get('tab', '')
        if not google.TAB.fullmatch(tab):
            raise ValueError()
        deadline = time.monotonic() + 25
        while connected_clock() < deadline:
            view = accounts_page(browser, tab)
            pair = google.identifier_controls(view.get('controls', []))
            if view.get('origin') == google.ACCOUNTS and pair:
                google.submit_identifier(browser, tab, pair, config['email'])
                break
            time.sleep(.4)
        else:
            raise google.Outcome('needs_human', 'form_changed')
        login(browser, tab, config['email'], secret, identified=True, clock=connected_clock,
              on_verification=lambda: google.send({'status':'verifying'}))
        google.send({'status':'signed_in'})
        payload = google.receive(8192, 45)
        consent(browser, payload['url'], config, request['service'], clock=connected_clock)
        returned = True
    finally:
        wipe(secret)
        if browser is not None:
            try:
                browser.discard()
            except Exception as failure:
                raise TeardownFailed() from failure
    # A terminal status is also the teardown acknowledgement: the gateway may
    # release the member-wide live marker only after the browser is discarded.
    if returned:
        google.send({'status':'returned'})


def report():
    try:
        main()
    except TeardownFailed:
        # Closing the guest stream without a terminal status makes the gateway
        # hold its live marker for the fail-safe grace period.
        return
    except google.Outcome as failure:
        google.send({'status': 'blocked' if failure.public['status'] == 'blocked' else 'needs_human'})
    except Exception:
        google.send({'status':'unavailable'})


if __name__ == '__main__':
    os.umask(0o077)
    report()
