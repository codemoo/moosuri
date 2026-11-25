"""Owner-side integration installed next to the official Kiri Chrome guest helper."""
import json
import os
import re
import select
import sys
import time
from urllib.parse import urlsplit
from uuid import UUID

import fresh_browser as fresh
import secure_google as google


MARKER = 'moosuri-workspace.json'
CONNECTOR = '/#connections/gmail'


def member_origin():
    raw = fresh._read(fresh.BASE / 'moosuri-workspace-origin')
    origin = raw.decode('ascii').rstrip('\n')
    parts = urlsplit(origin)
    if (len(raw) > 260 or parts.scheme != 'https' or not parts.hostname or
            not re.fullmatch(r'[a-z0-9](?:[a-z0-9.-]*[a-z0-9])?', parts.hostname) or
            origin != 'https://' + parts.hostname or '.' not in parts.hostname):
        raise ValueError('invalid member origin')
    return origin


def workspace_id(value):
    return isinstance(value, str) and len(value) == 36 and value == str(UUID(value)) and UUID(value).int != 0


def capture_workspace(browser, workspace, tab):
    job = fresh._path(browser.id)
    with fresh._job_lock(job):
        state = fresh._state(job)
        if state['phase'] != 'staging' or fresh._expired(state):
            raise ValueError('expired')
        flags = os.O_WRONLY | os.O_CREAT | os.O_EXCL | os.O_NOFOLLOW
        with os.fdopen(os.open(job / MARKER, flags, 0o600), 'w') as out:
            json.dump({'workspace': workspace, 'tab': tab}, out)
            out.flush()
            os.fsync(out.fileno())


def open_connector(browser, origin, *, clock=time.monotonic, sleep=time.sleep):
    tab = browser.call('open', url=origin + CONNECTOR).get('tab', '')
    if not google.TAB.fullmatch(tab):
        raise ValueError('connector tab unavailable')
    deadline = clock() + 10
    while clock() < deadline:
        page = browser.call('controls', tab=tab)
        if page.get('origin') == origin:
            return tab
        if page.get('origin') not in (None, '', 'about:blank', 'null') or page.get('error') not in (None, 'page_loading', 'page_unavailable'):
            break
        sleep(.25)
    raise ValueError('connector origin unconfirmed')


def owner_clock():
    # After the identifier reply there is no input until the retention ACK.
    # An EOF means the initiating bridge disappeared.
    if select.select([sys.stdin.fileno()], [], [], 0)[0]:
        raise google.Outcome('cancelled', 'secure_input_cancelled')
    return time.monotonic()


def signin(*, create=fresh.FreshBrowser.create, verify=google.verify, origin_fn=member_origin,
           capture=capture_workspace, write=google.send, receive=google.receive, monitor=owner_clock):
    browser = None
    retained = False
    announced = False
    end = time.monotonic() + 210
    workspace = None

    def identifier():
        nonlocal workspace
        write({'prompt': 'identifier', 'origin': google.ACCOUNTS})
        reply = receive(1024, 90)
        if not isinstance(reply, dict) or set(reply) != {'workspace', 'value'} or not workspace_id(reply['workspace']):
            raise ValueError('invalid workspace request')
        workspace = reply['workspace']
        return reply['value']

    def clock():
        now = monitor()
        if now >= end:
            raise google.Outcome('error', 'sign_in_timeout')
        return now

    try:
        origin = origin_fn()
        browser = create()
        home = verify(browser, identifier, clock=clock,
                      on_phone=lambda notice: write({'notice': notice, 'origin': google.ACCOUNTS}))
        if workspace is None:
            raise ValueError('unbound workspace')
        tab = open_connector(browser, origin, clock=clock)
        if home == tab or browser.call('close', tab=home) != {'closed': True}:
            raise ValueError('verified tab did not close')
        clock()
        capture(browser, workspace, tab)
        write({'result': {'status': 'ready', 'continuation': 'member_workspace'}})
        announced = True
        if receive(256, 10) != {'retain': workspace}:
            raise ValueError('retention was not acknowledged')
        browser.publish()
        write({'retained': True})
        retained = True
    except google.Outcome as failure:
        result = failure.public
    except Exception:
        result = {'status': 'error', 'reason': 'browser_unavailable'}
    finally:
        if browser is not None and not retained:
            try:
                browser.discard()
            except Exception:
                result = {'status': 'error', 'reason': 'cleanup_failed'}
    if not retained and not announced:
        write({'result': result})


def resume(workspace, *, ending=False):
    if not workspace_id(workspace):
        raise ValueError('invalid workspace')
    matches = []
    for job in fresh._root().iterdir():
        if not fresh.JOB_ID.fullmatch(job.name):
            continue
        try:
            with fresh._job_lock(job):
                state = fresh._state(job)
                if state['phase'] != 'published' or (not ending and fresh._expired(state)):
                    continue
                marker = json.loads(fresh._read(job / MARKER))
                if (set(marker) == {'workspace', 'tab'} and marker['workspace'] == workspace and
                        isinstance(marker['tab'], str) and google.TAB.fullmatch(marker['tab'])):
                    matches.append((job.name, marker['tab']))
        except (OSError, ValueError, TypeError, KeyError, fresh.Failure):
            continue
    if len(matches) != 1:
        raise ValueError('workspace unavailable')
    return matches[0]


def main(args):
    os.umask(0o077)
    if args == ['signin']:
        signin()
    elif len(args) == 2 and args[0] in {'resume', 'status', 'finish'}:
        session, tab = resume(args[1], ending=args[0] == 'finish')
        if args[0] == 'finish':
            fresh.FreshBrowser(session).discard()
            google.send({'finished': True})
            return
        page = fresh.use(session, {'op': 'controls', 'tab': tab})
        origin = member_origin()
        if page.get('origin') not in {origin, google.ACCOUNTS}:
            raise ValueError('workspace unavailable')
        if args[0] == 'status':
            google.send({'status': 'available', 'member_page': page['origin'] == origin})
        else:
            # Same owner-only session protocol as kiri-chrome google-sign-in.
            # Member socket clients never receive this local capability.
            google.send({'session': session, 'tab': tab})
    else:
        raise ValueError('unsupported operation')


if __name__ == '__main__':
    try:
        main(sys.argv[1:])
    except (ValueError, KeyError, OSError, fresh.Failure):
        google.send({'error': 'workspace_unavailable'})
        sys.exit(1)
