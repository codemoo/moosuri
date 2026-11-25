"""Run on Linux with the enrolled kiri-chrome guest scripts on PYTHONPATH."""
import json
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch
from uuid import uuid4

import moosuri_workspace as flow


ORIGIN = 'https://member.example.test'
TAB = 'B' * 32


class Browser:
    def __init__(self, reached=ORIGIN):
        self.reached = reached
        self.calls = []
        self.published = self.discarded = False

    def call(self, op, **options):
        self.calls.append((op, options))
        if op == 'close':
            return {'closed': True}
        return {'tab': TAB} if op == 'open' else {'origin': self.reached}

    def publish(self):
        self.published = True

    def discard(self):
        self.discarded = True


class Continuation(unittest.TestCase):
    def run_flow(self, browser, *, stop=None, capture_error=False):
        replies, markers = [], []
        job = str(uuid4())

        def verify(instance, ask, on_phone, clock):
            self.assertIs(instance, browser)
            self.assertEqual(ask(), 'member@example.test')
            if stop:
                raise flow.google.Outcome(*stop)
            return 'A' * 32

        def capture(instance, workspace, tab):
            if capture_error:
                raise OSError()
            markers.append((instance, workspace, tab))

        incoming = iter([{'value': 'member@example.test', 'workspace': job}, {'retain': job}])
        flow.signin(create=lambda: browser, verify=verify, origin_fn=lambda: ORIGIN,
                    capture=capture, write=replies.append, receive=lambda *_: next(incoming),
                    monitor=flow.time.monotonic)
        return replies, markers, job

    def test_verified_member_opens_connector_in_same_browser(self):
        browser = Browser()
        replies, markers, job = self.run_flow(browser)
        self.assertEqual(browser.calls, [('open', {'url': ORIGIN + '/#connections/gmail'}), ('controls', {'tab': TAB}), ('close', {'tab':'A'*32})])
        self.assertEqual(markers, [(browser, job, TAB)])
        self.assertTrue(browser.published)
        self.assertFalse(browser.discarded)
        self.assertEqual(replies[-2:], [{'result': {'status': 'ready', 'continuation': 'member_workspace'}}, {'retained': True}])

    def test_challenges_do_not_leave_reusable_profiles(self):
        for outcome in [('needs_human', 'provider_sign_in_required'), ('needs_human', 'verification'),
                        ('blocked', 'browser_rejected')]:
            with self.subTest(outcome=outcome):
                browser = Browser()
                replies, markers, _ = self.run_flow(browser, stop=outcome)
                self.assertEqual(replies[-1], {'result': dict(zip(('status', 'reason'), outcome))})
                self.assertEqual(browser.calls, [])
                self.assertEqual(markers, [])
                self.assertTrue(browser.discarded)

    def test_cross_origin_or_storage_failure_closes_browser(self):
        for browser, fault in [(Browser('https://other.example.test'), False), (Browser(), True)]:
            replies, markers, _ = self.run_flow(browser, capture_error=fault)
            self.assertEqual(replies[-1]['result']['status'], 'error')
            self.assertTrue(browser.discarded)
            self.assertEqual(markers, [])

    def test_owner_enrollment_requires_exact_https_origin(self):
        with tempfile.TemporaryDirectory() as directory, patch.object(flow.fresh, 'BASE', Path(directory)):
            config = Path(directory) / 'moosuri-workspace-origin'
            for value in [ORIGIN, ORIGIN+'/path', ORIGIN+'#token', 'http://member.example.test', 'https://a@member.example.test']:
                config.write_text(value+'\n')
                config.chmod(0o600)
                if value == ORIGIN:
                    self.assertEqual(flow.member_origin(), ORIGIN)
                else:
                    with self.assertRaises(ValueError):
                        flow.member_origin()
            config.chmod(0o644)
            with self.assertRaises(flow.fresh.Failure):
                flow.member_origin()

    def test_only_published_unexpired_owner_job_can_resume(self):
        with tempfile.TemporaryDirectory() as directory, patch.object(flow.fresh, 'BASE', Path(directory)):
            root = flow.fresh._root(create=True)
            job = root / ('a' * 32)
            job.mkdir(mode=0o700)
            workspace = str(uuid4())
            marker = job / flow.MARKER
            marker.write_text(json.dumps({'workspace': workspace, 'tab': TAB}))
            marker.chmod(0o600)
            with flow.fresh._job_lock(job, create=True):
                state = {'phase': 'published', 'closing': False, 'image': 'sha256:'+'0'*64,
                         'policy': {}, 'expires': 4_000_000_000}
                flow.fresh._write_state(job, state)
            self.assertEqual(flow.resume(workspace), (job.name, TAB))
            with self.assertRaises(ValueError):
                flow.resume(str(uuid4()))
            broken = root / ('b' * 32)
            broken.mkdir(mode=0o700)
            with flow.fresh._job_lock(broken, create=True):
                flow.fresh._write_state(broken, {})
            self.assertEqual(flow.resume(workspace), (job.name, TAB))
            for change in [{'phase':'staging'}, {'closing':True}, {'expires':1}]:
                flow.fresh._write_state(job,{**state, **change})
                with self.assertRaises(ValueError):
                    flow.resume(workspace)
                if change.get('phase') != 'staging':
                    self.assertEqual(flow.resume(workspace, ending=True), (job.name, TAB))

    def test_missing_parent_ack_discards_staging_profile(self):
        browser = Browser()
        job = str(uuid4())
        replies = []
        incoming = iter([{'value':'member@example.test', 'workspace':job}, {'retain':str(uuid4())}])
        def verify(instance, ask, **kwargs):
            ask()
            return 'A'*32
        flow.signin(create=lambda:browser, verify=verify, origin_fn=lambda:ORIGIN,
                    capture=lambda *args:None, write=replies.append, receive=lambda *_:next(incoming),
                    monitor=flow.time.monotonic)
        self.assertTrue(browser.discarded)
        self.assertFalse(browser.published)
        self.assertEqual(len([r for r in replies if 'result' in r]), 1)


if __name__ == '__main__':
    unittest.main()
