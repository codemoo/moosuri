from pathlib import Path
import time
import importlib.util
import base64, json, secrets, uuid
from cryptography.hazmat.primitives.asymmetric import rsa, padding
from cryptography.hazmat.primitives import serialization, hashes
from cryptography.hazmat.primitives.ciphers.aead import AESGCM
from playwright.sync_api import expect

source=Path(__file__).with_name("browser-fullstack.py")
spec=importlib.util.spec_from_file_location("harness",source)
h=importlib.util.module_from_spec(spec);spec.loader.exec_module(h)
def exercise(context, origin, vault_origin, email, login_password, _):
    app=context.new_page();app.goto(origin+"/app")
    app.get_by_label("Email").fill(email);app.get_by_label("Password").fill(login_password)
    app.get_by_role("button",name="Sign in").click()
    app.get_by_role("button",name="Open conversations").click()
    with app.expect_popup() as popup: app.get_by_role("dialog").get_by_role("button",name="볼트 열기",exact=True).click()
    vault=popup.value
    vault.get_by_role("heading",name="내 볼트 만들기").wait_for()
    account=vault.evaluate("async()=> (await (await fetch('/session/current')).json()).owner.account")
    key=rsa.generate_private_key(public_exponent=65537,key_size=3072)
    public=base64.b64encode(key.public_key().public_bytes(serialization.Encoding.DER,serialization.PublicFormat.SubjectPublicKeyInfo)).decode()
    context.route(vault_origin+"/session/kiri-recipient",lambda route:route.fulfill(json={'account':account,'email':email,'spki':public}))
    job=str(uuid.uuid4())
    app.evaluate("""([origin,account,email,job])=>{
        window.testJob=job;window.packets=[];
        window.addEventListener('message',e=>{
          if(e.origin!==origin||e.data?.account!==account)return;
          if(e.data.kind==='moosuri-vault-google-context')e.source.postMessage({kind:'moosuri-vault-google-account',account,email,job:window.testJob},origin);
          if(e.data.kind==='moosuri-vault-kiri-sealed')window.packets.push(e.data);
        });
    }""",[vault_origin,account,email,job])
    vault.reload()
    password=secrets.token_hex(14)
    vault.get_by_label("볼트 비밀번호",exact=True).fill(password)
    vault.get_by_role("button",name="만들기",exact=True).click()
    vault.get_by_label("기록한 복구 키").fill(vault.get_by_label("새 복구 키").input_value())
    vault.get_by_role("button",name="저장하고 열기").click()
    field=vault.get_by_label('Google 계정 비밀번호')
    expect(field).to_be_visible()
    secret=secrets.token_hex(17);field.fill(secret)
    vault.get_by_role("button",name="볼트에 저장하고 계속").click()
    def wait_for_packet(count):
        for _ in range(60):
            if app.evaluate("window.packets.length")==count:return
            time.sleep(.2)
        raise AssertionError("vault did not send the expected number of encrypted packets")
    wait_for_packet(1)
    def verify(index,job):
        p=app.evaluate("(i)=>window.packets[i]",index)
        assert p['job']==job and secret not in json.dumps(p)
        label=('moosuri-google-v1\n'+job+'\n'+account+'\n'+email).encode()
        e=p['envelope']
        aes=key.decrypt(base64.b64decode(e['key']),padding.OAEP(mgf=padding.MGF1(hashes.SHA256()),algorithm=hashes.SHA256(),label=label))
        assert AESGCM(aes).decrypt(base64.b64decode(e['iv']),base64.b64decode(e['body']),label)==secret.encode()
    verify(0,job)
    print('PASS missing credential saved by WASM and encrypted for exact Kiri job',flush=True)
    job=str(uuid.uuid4());app.evaluate("j=>{window.testJob=j}",job)
    vault.reload();vault.get_by_label('볼트 비밀번호',exact=True).fill(password);vault.get_by_role('button',name='열기',exact=True).click()
    wait_for_packet(2);verify(1,job)
    expect(vault.get_by_label('Google 계정 비밀번호')).to_have_count(0)
    print('PASS existing credential reused after relock and reload without password reentry',flush=True)
h.exercise=exercise;h.main()
