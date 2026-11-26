"""Synthetic login scenarios. Uses the same account recognition helpers as the Kiri guest."""
import base64
import itertools
import json
import unittest
from unittest.mock import Mock, call, patch
from cryptography.hazmat.primitives.asymmetric import rsa, padding
from cryptography.hazmat.primitives import hashes, serialization
from cryptography.hazmat.primitives.ciphers.aead import AESGCM
import google_connection as flow

EMAIL = 'someone@example.test'
TAB = 'b' * 32
NEXT = dict(role='button', label='Next', ref='3'*64)
FIELD = dict(role='textbox', label='Email', type='email', editable=True, credential=True, ref='1'*64)
PASS = dict(FIELD, label='Password', type='password', ref='2'*64)
HOME = dict(origin=flow.google.ACCOUNT_HOME, controls=[dict(role='button', label='Google Account '+EMAIL)])
def page(*controls):
    return dict(origin=flow.google.ACCOUNTS, controls=list(controls))


class Browser:
    def __init__(self, views):
        self.views = iter(views)
        self.actions = []
    def call(self, op, **kwargs):
        if op == 'controls':
            return next(self.views)
        self.actions.append((op, kwargs))
        return {'acted': True, 'tab': TAB}


class Connection(unittest.TestCase):
    @classmethod
    def seal(cls, label, secret=b'synthetic'):
        aes = AESGCM.generate_key(bit_length=256)
        iv = bytes(range(12))
        wrapped = cls.key.public_key().encrypt(aes,padding.OAEP(mgf=padding.MGF1(hashes.SHA256()),algorithm=hashes.SHA256(),label=label))
        return {k:base64.b64encode(v).decode() for k,v in {
            'key':wrapped,'iv':iv,'body':AESGCM(aes).encrypt(iv,secret,label)}.items()}
    @classmethod
    def setUpClass(cls):
        cls.key = rsa.generate_private_key(public_exponent=65537,key_size=3072)
        cls.label = flow.binding('attempt','account',EMAIL)
        cls.packet = cls.seal(cls.label)
    def run_login(self, browser):
        flow.login(browser,TAB,EMAIL,bytearray(b'synthetic'),clock=itertools.count().__next__,sleep=lambda _:None)
    def test_exact_account_password_and_home(self):
        browser=Browser([page(FIELD,NEXT),page(PASS,NEXT,dict(label=EMAIL)),HOME])
        self.run_login(browser)
        self.assertEqual([v['text'] for op,v in browser.actions if op=='fill'],[EMAIL,'synthetic'])
    def test_account_chip_arrives_after_password_field(self):
        browser=Browser([page(FIELD,NEXT),page(PASS,NEXT),page(PASS,NEXT,dict(label=EMAIL)),HOME])
        self.run_login(browser)
        self.assertEqual([v['text'] for op,v in browser.actions if op=='fill'],[EMAIL,'synthetic'])
    def test_wrong_recipient_never_receives_password(self):
        browser=Browser([page(FIELD,NEXT),page(PASS,NEXT,dict(label='other@example.test'))])
        with self.assertRaises(flow.google.Outcome):
            self.run_login(browser)
        self.assertEqual(len([1 for op,_ in browser.actions if op=='fill']),1)
    def test_password_not_retried_after_failure(self):
        form=page(PASS,NEXT,dict(label=EMAIL))
        browser=Browser([page(FIELD,NEXT)]+[form]*100)
        with self.assertRaises(flow.google.Outcome):
            self.run_login(browser)
        self.assertEqual(len([1 for op,_ in browser.actions if op=='fill']),2)
    def test_challenge_and_rejection_stop(self):
        for view in ({'reason':'verification'},{'reason':'browser_rejected'},dict(origin='https://other.example.test')):
            b=Browser([view])
            with self.assertRaises(flow.google.Outcome):
                self.run_login(b)
            self.assertEqual(b.actions,[])
    def test_closed_gateway_stops_before_password_submission(self):
        browser=Browser([page(PASS,NEXT,dict(label=EMAIL))])
        with patch.object(flow.select,'select',side_effect=[([],[],[]),([0],[],[])]):
            with self.assertRaises(ValueError):
                flow.login(browser,TAB,EMAIL,bytearray(b'synthetic'),identified=True,
                           clock=flow.connected_clock,sleep=lambda _:None)
        self.assertEqual(browser.actions,[])
    def test_different_job_cannot_unwrap(self):
        with self.assertRaises(ValueError):
            flow.decrypt(self.packet,self.key,flow.binding('different','account',EMAIL))
    def test_invalid_plaintext_buffer_is_wiped(self):
        value = bytearray(b'bad\0secret')
        with self.assertRaises(ValueError):
            flow.validate_secret(value)
        self.assertEqual(value, bytearray(len(value)))
    def test_invalid_packet_stops_before_browser_creation(self):
        job = '00000000-0000-4000-8000-000000000001'
        config = json.dumps({'account':'account','email':EMAIL}).encode()
        private = self.key.private_bytes(
            serialization.Encoding.PEM,
            serialization.PrivateFormat.PKCS8,
            serialization.NoEncryption(),
        )
        request = {'protocol':flow.PROTOCOL,'job':job,'account':'account','email':EMAIL,'service':'gmail'}
        with (patch.object(flow.fresh,'_read',side_effect=[config,private]),
              patch.object(flow.google,'receive',side_effect=[request,{'key':'bad','iv':'bad','body':'bad'}]),
              patch.object(flow.google,'send') as sent,
              patch.object(flow.fresh.FreshBrowser,'create') as create):
            with self.assertRaises(Exception):
                flow.main()
        sent.assert_called_once_with({'status':'vault','protocol':flow.PROTOCOL})
        create.assert_not_called()
    def test_valid_packet_is_confirmed_before_browser_creation(self):
        job = '00000000-0000-4000-8000-000000000001'
        config = json.dumps({'account':'account','email':EMAIL}).encode()
        private = self.key.private_bytes(
            serialization.Encoding.PEM,
            serialization.PrivateFormat.PKCS8,
            serialization.NoEncryption(),
        )
        request = {'protocol':flow.PROTOCOL,'job':job,'account':'account','email':EMAIL,'service':'gmail'}
        packet = self.seal(flow.binding(job,'account',EMAIL))
        with (patch.object(flow.fresh,'_read',side_effect=[config,private]),
              patch.object(flow.google,'receive',side_effect=[request,packet,{'status':'proceed'}]),
              patch.object(flow.google,'send') as sent,
              patch.object(flow.fresh.FreshBrowser,'create',side_effect=RuntimeError('browser_started')) as create):
            with self.assertRaisesRegex(RuntimeError,'browser_started'):
                flow.main()
        self.assertEqual(sent.call_args_list,[call({'status':'vault','protocol':flow.PROTOCOL}),call({'status':'verified'})])
        create.assert_called_once_with()
    def test_gateway_release_is_required_before_browser_creation(self):
        job = '00000000-0000-4000-8000-000000000001'
        config = json.dumps({'account':'account','email':EMAIL}).encode()
        private = self.key.private_bytes(
            serialization.Encoding.PEM,
            serialization.PrivateFormat.PKCS8,
            serialization.NoEncryption(),
        )
        request = {'protocol':flow.PROTOCOL,'job':job,'account':'account','email':EMAIL,'service':'gmail'}
        packet = self.seal(flow.binding(job,'account',EMAIL))
        with (patch.object(flow.fresh,'_read',side_effect=[config,private]),
              patch.object(flow.google,'receive',side_effect=[request,packet,{'status':'stop'}]),
              patch.object(flow.google,'send') as sent,
              patch.object(flow.fresh.FreshBrowser,'create') as create):
            with self.assertRaises(ValueError):
                flow.main()
        self.assertEqual(sent.call_args_list,[call({'status':'vault','protocol':flow.PROTOCOL}),call({'status':'verified'})])
        create.assert_not_called()
    def test_closed_gateway_during_browser_creation_discards_before_open(self):
        job = '00000000-0000-4000-8000-000000000001'
        config = json.dumps({'account':'account','email':EMAIL}).encode()
        private = self.key.private_bytes(
            serialization.Encoding.PEM,
            serialization.PrivateFormat.PKCS8,
            serialization.NoEncryption(),
        )
        request = {'protocol':flow.PROTOCOL,'job':job,'account':'account','email':EMAIL,'service':'gmail'}
        packet = self.seal(flow.binding(job,'account',EMAIL))
        browser = Browser([])
        browser.discard = Mock()
        with (patch.object(flow.fresh,'_read',side_effect=[config,private]),
              patch.object(flow.google,'receive',side_effect=[request,packet,{'status':'proceed'}]),
              patch.object(flow.google,'send') as sent,
              patch.object(flow.fresh.FreshBrowser,'create',return_value=browser),
              patch.object(flow,'connected_clock',side_effect=ValueError('connection closed'))):
            with self.assertRaisesRegex(ValueError,'connection closed'):
                flow.main()
        self.assertEqual(sent.call_args_list,[call({'status':'vault','protocol':flow.PROTOCOL}),call({'status':'verified'})])
        self.assertEqual(browser.actions,[])
        browser.discard.assert_called_once_with()
    def test_plaintext_source_buffer_is_overwritten_when_browser_creation_fails(self):
        job = '00000000-0000-4000-8000-000000000001'
        config = json.dumps({'account':'account','email':EMAIL}).encode()
        private = self.key.private_bytes(
            serialization.Encoding.PEM,
            serialization.PrivateFormat.PKCS8,
            serialization.NoEncryption(),
        )
        request = {'protocol':flow.PROTOCOL,'job':job,'account':'account','email':EMAIL,'service':'gmail'}
        secret = bytearray(b'synthetic')
        with (patch.object(flow.fresh,'_read',side_effect=[config,private]),
              patch.object(flow.google,'receive',side_effect=[request,{}, {'status':'proceed'}]),
              patch.object(flow.google,'send'),
              patch.object(flow,'decrypt',return_value=secret),
              patch.object(flow.fresh.FreshBrowser,'create',side_effect=RuntimeError('browser_started'))):
            with self.assertRaisesRegex(RuntimeError,'browser_started'):
                flow.main()
        self.assertEqual(secret,bytearray(len(secret)))
    def test_old_gateway_protocol_stops_before_vault_or_browser(self):
        job = '00000000-0000-4000-8000-000000000001'
        config = json.dumps({'account':'account','email':EMAIL}).encode()
        request = {'job':job,'account':'account','email':EMAIL,'service':'gmail'}
        with (patch.object(flow.fresh,'_read',return_value=config),
              patch.object(flow.google,'receive',return_value=request),
              patch.object(flow.google,'send') as sent,
              patch.object(flow.fresh.FreshBrowser,'create') as create):
            with self.assertRaises(ValueError):
                flow.main()
        sent.assert_not_called()
        create.assert_not_called()
    def test_returned_is_sent_after_browser_discard(self):
        job = '00000000-0000-4000-8000-000000000001'
        config = json.dumps({'account':'account','email':EMAIL}).encode()
        private = self.key.private_bytes(
            serialization.Encoding.PEM,
            serialization.PrivateFormat.PKCS8,
            serialization.NoEncryption(),
        )
        request = {'protocol':flow.PROTOCOL,'job':job,'account':'account','email':EMAIL,'service':'gmail'}
        packet = self.seal(flow.binding(job,'account',EMAIL))
        browser = Browser([])
        events = []
        browser.discard = lambda: events.append('discard')
        with (patch.object(flow.fresh,'_read',side_effect=[config,private]),
              patch.object(flow.google,'receive',side_effect=[request,packet,{'status':'proceed'},{'url':'synthetic'}]),
              patch.object(flow.google,'send',side_effect=lambda value: events.append(value['status'])),
              patch.object(flow.fresh.FreshBrowser,'create',return_value=browser),
              patch.object(flow,'connected_clock',side_effect=itertools.count().__next__),
              patch.object(flow,'accounts_page',return_value=page(FIELD,NEXT)),
              patch.object(flow.google,'submit_identifier'),
              patch.object(flow,'login'),
              patch.object(flow,'consent')):
            flow.main()
        self.assertLess(events.index('discard'),events.index('returned'))
    def test_discard_failure_never_acknowledges_teardown(self):
        job = '00000000-0000-4000-8000-000000000001'
        config = json.dumps({'account':'account','email':EMAIL}).encode()
        private = self.key.private_bytes(
            serialization.Encoding.PEM,
            serialization.PrivateFormat.PKCS8,
            serialization.NoEncryption(),
        )
        request = {'protocol':flow.PROTOCOL,'job':job,'account':'account','email':EMAIL,'service':'gmail'}
        packet = self.seal(flow.binding(job,'account',EMAIL))
        browser = Browser([])
        browser.discard = Mock(side_effect=RuntimeError('still_live'))
        statuses = []
        with (patch.object(flow.fresh,'_read',side_effect=[config,private]),
              patch.object(flow.google,'receive',side_effect=[request,packet,{'status':'proceed'},{'url':'synthetic'}]),
              patch.object(flow.google,'send',side_effect=lambda value: statuses.append(value['status'])),
              patch.object(flow.fresh.FreshBrowser,'create',return_value=browser),
              patch.object(flow,'connected_clock',side_effect=itertools.count().__next__),
              patch.object(flow,'accounts_page',return_value=page(FIELD,NEXT)),
              patch.object(flow.google,'submit_identifier'),
              patch.object(flow,'login'),
              patch.object(flow,'consent')):
            with self.assertRaises(flow.TeardownFailed):
                flow.main()
        browser.discard.assert_called_once_with()
        self.assertNotIn('returned',statuses)
        self.assertNotIn('unavailable',statuses)
    def test_teardown_failure_report_closes_without_terminal_status(self):
        with (patch.object(flow,'main',side_effect=flow.TeardownFailed()),
              patch.object(flow.google,'send') as sent):
            flow.report()
        sent.assert_not_called()
    def test_consent_rejects_other_callback_before_navigation(self):
        b=Browser([])
        with self.assertRaises(ValueError):
            flow.consent(b,'https://accounts.google.com/o/oauth2/v2/auth?redirect_uri=https://other.test',{'origin':'https://member.test','email':EMAIL},'gmail')
        self.assertEqual(b.actions,[])
