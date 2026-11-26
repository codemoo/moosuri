"""Synthetic login scenarios. Uses the same account recognition helpers as the Kiri guest."""
import base64
import itertools
import unittest
from cryptography.hazmat.primitives.asymmetric import rsa, padding
from cryptography.hazmat.primitives import hashes
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
    def setUpClass(cls):
        cls.key = rsa.generate_private_key(public_exponent=65537,key_size=3072)
        cls.label = flow.binding('attempt','account',EMAIL)
        aes = AESGCM.generate_key(bit_length=256)
        iv = bytes(range(12))
        wrapped = cls.key.public_key().encrypt(aes,padding.OAEP(mgf=padding.MGF1(hashes.SHA256()),algorithm=hashes.SHA256(),label=cls.label))
        cls.packet = {k:base64.b64encode(v).decode() for k,v in {
            'key':wrapped,'iv':iv,'body':AESGCM(aes).encrypt(iv,b'synthetic',cls.label)}.items()}
    def run_login(self, browser):
        flow.login(browser,TAB,EMAIL,self.packet,self.key,self.label,clock=itertools.count().__next__,sleep=lambda _:None)
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
    def test_different_job_cannot_unwrap(self):
        with self.assertRaises(ValueError):
            flow.decrypt(self.packet,self.key,flow.binding('different','account',EMAIL))
    def test_consent_rejects_other_callback_before_navigation(self):
        b=Browser([])
        with self.assertRaises(ValueError):
            flow.consent(b,'https://accounts.google.com/o/oauth2/v2/auth?redirect_uri=https://other.test',{'origin':'https://member.test','email':EMAIL},'gmail')
        self.assertEqual(b.actions,[])
