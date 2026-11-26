use super::*;
use base64::{Engine, engine::general_purpose::STANDARD};
use serde_json::json;
use std::process::Stdio;
use tokio::io::AsyncWriteExt;
use tokio::process::Command;

pub(super) fn binding() -> Binding {
    Binding {
        installation: uuid::Uuid::new_v4(),
        account: uuid::Uuid::new_v4(),
        tenant: uuid::Uuid::new_v4(),
        volume: uuid::Uuid::new_v4(),
        generation: 4,
        epoch: 2,
        challenge: uuid::Uuid::new_v4(),
        email: "member@example.test".into(),
        origin: "https://moosuri.example.test".into(),
    }
}

// Node's actual WebCrypto API independently produces the browser-vault envelope.
// The only plaintext is synthetic, sent over stdin and never in process arguments.
async fn seal(recipient: &Recipient, label: &[u8], secret: &[u8]) -> Envelope {
    let script = r#"const {webcrypto}=require('node:crypto');let chunks=[];process.stdin.on('data',x=>chunks.push(x));process.stdin.on('end',async()=>{
      const v=JSON.parse(Buffer.concat(chunks)); const c=webcrypto.subtle;
      const publicKey=await c.importKey('spki',Buffer.from(v.spki,'base64'),{name:'RSA-OAEP',hash:'SHA-256'},false,['encrypt']);
      const key=await c.generateKey({name:'AES-GCM',length:256},true,['encrypt']);
      const raw=await c.exportKey('raw',key), label=Buffer.from(v.label,'base64'), iv=webcrypto.getRandomValues(new Uint8Array(12));
      const wrapped=await c.encrypt({name:'RSA-OAEP',label},publicKey,raw);
      const body=await c.encrypt({name:'AES-GCM',iv,additionalData:label},key,Buffer.from(v.secret,'base64'));
      const encode=x=>Buffer.from(x).toString('base64');
      process.stdout.write(JSON.stringify({key:encode(wrapped),iv:encode(iv),body:encode(body)}));
    });"#;
    let mut child = Command::new("node")
        .args(["-e", script])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .kill_on_drop(true)
        .spawn()
        .expect("Node WebCrypto required on Linux test host");
    child.stdin.take().unwrap().write_all(json!({"spki":recipient.spki,"label":STANDARD.encode(label),"secret":STANDARD.encode(secret)}).to_string().as_bytes()).await.unwrap();
    let result = child.wait_with_output().await.unwrap();
    assert!(result.status.success());
    serde_json::from_slice(&result.stdout).unwrap()
}

#[tokio::test]
async fn browser_webcrypto_envelope_opens_once_and_private_key_is_removed() {
    let b = binding();
    let receiver = Receiver::create(b.clone()).await.unwrap();
    let directory = receiver.directory().to_owned();
    use std::os::unix::fs::MetadataExt;
    assert_eq!(
        std::fs::metadata(directory.join("key.pem")).unwrap().mode() & 0o777,
        0o600
    );
    let selected = "selected@example.test";
    let packet = seal(
        &receiver.recipient(),
        &b.label(selected).unwrap(),
        b"synthetic-only-password",
    )
    .await;
    let actual = receiver.open(selected, packet).await.unwrap();
    assert_eq!(actual.as_slice(), b"synthetic-only-password");
    assert!(!directory.exists());
}

#[tokio::test]
async fn every_allocation_and_selected_account_field_is_authenticated() {
    let original = binding();
    for field in 0..10 {
        let mut changed = original.clone();
        let mut selected = "selected@example.test";
        match field {
            0 => changed.installation = uuid::Uuid::new_v4(),
            1 => changed.account = uuid::Uuid::new_v4(),
            2 => changed.tenant = uuid::Uuid::new_v4(),
            3 => changed.volume = uuid::Uuid::new_v4(),
            4 => changed.generation += 1,
            5 => changed.epoch += 1,
            6 => changed.challenge = uuid::Uuid::new_v4(),
            7 => changed.email = "sibling@example.test".into(),
            8 => changed.origin = "https://other.example.test".into(),
            _ => selected = "different@example.test",
        }
        let receiver = Receiver::create(changed).await.unwrap();
        let packet = seal(
            &receiver.recipient(),
            &original.label("selected@example.test").unwrap(),
            b"synthetic-only-password",
        )
        .await;
        assert!(
            receiver.open(selected, packet).await.is_err(),
            "binding field {field}"
        );
    }
}

#[tokio::test]
async fn sibling_attempt_key_cannot_open_an_identically_bound_packet() {
    let b = binding();
    let first = Receiver::create(b.clone()).await.unwrap();
    let second = Receiver::create(b.clone()).await.unwrap();
    let packet = seal(
        &first.recipient(),
        &b.label("selected@example.test").unwrap(),
        b"synthetic-password",
    )
    .await;
    assert!(second.open("selected@example.test", packet).await.is_err());
}

#[tokio::test]
async fn tampered_ciphertext_and_invalid_passwords_are_rejected() {
    for secret in [
        Vec::new(),
        vec![0],
        vec![255],
        vec![b'x'; 4097],
        b"synthetic-password".to_vec(),
    ] {
        let b = binding();
        let receiver = Receiver::create(b.clone()).await.unwrap();
        let mut packet = seal(
            &receiver.recipient(),
            &b.label("selected@example.test").unwrap(),
            &secret,
        )
        .await;
        if secret == b"synthetic-password" {
            let mut bytes = STANDARD.decode(packet.body).unwrap();
            bytes[0] ^= 1;
            packet.body = STANDARD.encode(bytes);
        }
        assert!(
            receiver
                .open("selected@example.test", packet)
                .await
                .is_err()
        );
    }
}

#[test]
fn malformed_bindings_and_extra_protocol_fields_are_rejected() {
    let mut b = binding();
    for origin in [
        "http://example.test",
        "https://example.test/",
        "https://x@example.test",
        "https://example.test?x=1",
    ] {
        b.origin = origin.into();
        assert!(!b.valid());
    }
    assert!(!binding::valid_email("member\n@example.test"));
    let mut value = json!(binding());
    value["command"] = json!("not-allowed");
    assert!(serde_json::from_value::<Binding>(value).is_err());
}

fn consent(b: &Binding) -> consent::Consent {
    let client = "synthetic.apps.googleusercontent.com";
    let mut url = url::Url::parse("https://accounts.google.com/o/oauth2/v2/auth").unwrap();
    url.query_pairs_mut().extend_pairs([
        ("client_id", client),
        (
            "redirect_uri",
            &format!("{}/api/v1/mail/oauth/callback", b.origin),
        ),
        ("login_hint", "selected@example.test"),
        ("code_challenge_method", "S256"),
        ("response_type", "code"),
        ("include_granted_scopes", "false"),
        ("access_type", "offline"),
        ("state", "synthetic-state"),
        ("code_challenge", &"a".repeat(43)),
        ("scope", "https://www.googleapis.com/auth/gmail.readonly"),
    ]);
    consent::Consent {
        url: url.into(),
        client_id: client.into(),
        service: "gmail".into(),
    }
}

#[test]
fn consent_pins_callback_client_account_scope_and_pkce_before_navigation() {
    let b = binding();
    assert!(consent(&b).validate(&b, "selected@example.test").is_ok());
    for (field, value) in [
        (
            "redirect_uri",
            "https://other.example.test/api/v1/mail/oauth/callback",
        ),
        ("client_id", "other.apps.googleusercontent.com"),
        ("login_hint", "other@example.test"),
        ("scope", "https://mail.google.com/"),
        ("code_challenge_method", "plain"),
        ("include_granted_scopes", "true"),
    ] {
        let mut c = consent(&b);
        let mut url = url::Url::parse(&c.url).unwrap();
        let fields: Vec<_> = url
            .query_pairs()
            .into_owned()
            .map(|(k, v)| {
                if k == field {
                    (k, value.into())
                } else {
                    (k, v)
                }
            })
            .collect();
        url.query_pairs_mut().clear().extend_pairs(fields);
        c.url = url.into();
        assert!(c.validate(&b, "selected@example.test").is_err(), "{field}");
    }
    let mut c = consent(&b);
    c.url.push_str("&scope=https://mail.google.com/");
    assert!(c.validate(&b, "selected@example.test").is_err());
}

#[test]
fn callback_script_requires_exact_code_state_and_path() {
    let script = r#"const vm=require('node:vm'), assert=require('node:assert/strict');
const source=JSON.parse(process.argv[1]);
const callback='https://moosuri.example.test/api/v1/mail/oauth/callback';
for(const [suffix,expected] of [
 ['?code=synthetic&state=expected','returned'],
 ['?code=synthetic&state=other','blocked'],
 ['?error=access_denied&state=expected','blocked'],
 ['?code=synthetic&state=expected&state=other','blocked'],
 ['?state=expected','blocked'],
 ['/other?code=synthetic&state=expected','blocked']]) {
 const context=vm.createContext({URL, location:new URL(callback+suffix),document:{readyState:'complete'}});
 const fn=vm.runInContext('('+source+')',context);
 assert.equal(fn('selected@example.test',callback,'expected'),expected);
}
vm.runInNewContext('('+JSON.parse(process.argv[2])+')');
"#;
    let result = std::process::Command::new("node")
        .args([
            "-e",
            script,
            &serde_json::to_string(include_str!("consent.js")).unwrap(),
            &serde_json::to_string(include_str!("account.js")).unwrap(),
        ])
        .output()
        .expect("Node required for browser protocol tests");
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
}
