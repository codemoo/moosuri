//! Compatibility with the browser vault envelope, using standard crypto libraries.
use super::{Binding, Envelope, Error, Recipient, binding::PROTOCOL};
use base64::{Engine, engine::general_purpose::STANDARD};
use ring::aead;
use std::{path::Path, process::Stdio, time::Duration};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    process::Command,
};
use zeroize::Zeroizing;

pub struct Receiver {
    directory: tempfile::TempDir,
    binding: Binding,
    public: String,
}

async fn openssl(args: &[&str], input: &[u8], limit: usize) -> Result<Zeroizing<Vec<u8>>, Error> {
    let mut child = Command::new("/usr/bin/openssl")
        .env_clear()
        .env("PATH", "/usr/bin:/bin")
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .kill_on_drop(true)
        .spawn()
        .map_err(|_| Error::Unavailable)?;
    let mut stdin = child.stdin.take().ok_or(Error::Unavailable)?;
    let mut stdout = child.stdout.take().ok_or(Error::Unavailable)?;
    let result = tokio::time::timeout(Duration::from_secs(20), async {
        let write = async {
            stdin
                .write_all(input)
                .await
                .map_err(|_| Error::Unavailable)?;
            drop(stdin);
            Ok::<_, Error>(())
        };
        let read = async {
            let mut bytes = Zeroizing::new(Vec::new());
            (&mut stdout)
                .take(limit as u64 + 1)
                .read_to_end(&mut bytes)
                .await
                .map_err(|_| Error::Unavailable)?;
            if bytes.len() > limit {
                return Err(Error::Invalid);
            }
            Ok(bytes)
        };
        let (_, bytes) = tokio::try_join!(write, read)?;
        if !child
            .wait()
            .await
            .map_err(|_| Error::Unavailable)?
            .success()
        {
            return Err(Error::Invalid);
        }
        Ok(bytes)
    })
    .await
    .map_err(|_| Error::Unavailable)?;
    if result.is_err() {
        let _ = child.kill().await;
    }
    result
}

impl Receiver {
    #[cfg(test)]
    pub(super) fn directory(&self) -> &Path {
        self.directory.path()
    }
    pub async fn create(binding: Binding) -> Result<Self, Error> {
        if !binding.valid() {
            return Err(Error::Invalid);
        }
        // The worker's /tmp is a private tmpfs; never a host credential directory.
        let directory = tempfile::Builder::new()
            .prefix("moosuri-google-key-")
            .tempdir_in("/tmp")
            .map_err(|_| Error::Unavailable)?;
        let file = directory.path().join("key.pem");
        let path = file.to_str().ok_or(Error::Unavailable)?;
        // OpenSSL creates its private output with mode 0600 in our 0700 directory.
        openssl(
            &[
                "genpkey",
                "-algorithm",
                "RSA",
                "-pkeyopt",
                "rsa_keygen_bits:3072",
                "-out",
                path,
            ],
            &[],
            0,
        )
        .await?;
        let public = openssl(
            &["pkey", "-in", path, "-pubout", "-outform", "DER"],
            &[],
            1024,
        )
        .await?;
        Ok(Self {
            directory,
            binding,
            public: STANDARD.encode(public.as_slice()),
        })
    }

    pub fn recipient(&self) -> Recipient {
        Recipient {
            protocol: PROTOCOL.into(),
            binding: self.binding.clone(),
            spki: self.public.clone(),
        }
    }

    /// Consuming self prevents another delivery even after a failed unwrap.
    pub async fn open(
        self,
        google_email: &str,
        envelope: Envelope,
    ) -> Result<Zeroizing<Vec<u8>>, Error> {
        let label = self.binding.label(google_email)?;
        let wrapped = decode(&envelope.key, 384, 384)?;
        let nonce = decode(&envelope.iv, 12, 12)?;
        let mut body = Zeroizing::new(decode(&envelope.body, 17, 4112)?);
        let label_hex: String = label.iter().map(|b| format!("{b:02x}")).collect();
        let path = self.directory.path().join("key.pem");
        let secret = unwrap(&path, &wrapped, &label_hex).await?;
        if secret.len() != 32 {
            return Err(Error::Invalid);
        }
        let key = aead::LessSafeKey::new(
            aead::UnboundKey::new(&aead::AES_256_GCM, &secret).map_err(|_| Error::Invalid)?,
        );
        let nonce = aead::Nonce::try_assume_unique_for_key(&nonce).map_err(|_| Error::Invalid)?;
        let clear = key
            .open_in_place(nonce, aead::Aad::from(&label), &mut body)
            .map_err(|_| Error::Invalid)?;
        let len = clear.len();
        if len == 0 || len > 4096 || clear.contains(&0) || std::str::from_utf8(clear).is_err() {
            return Err(Error::Invalid);
        }
        body.truncate(len);
        Ok(body)
    }
}

async fn unwrap(path: &Path, wrapped: &[u8], label: &str) -> Result<Zeroizing<Vec<u8>>, Error> {
    openssl(
        &[
            "pkeyutl",
            "-decrypt",
            "-inkey",
            path.to_str().ok_or(Error::Unavailable)?,
            "-pkeyopt",
            "rsa_padding_mode:oaep",
            "-pkeyopt",
            "rsa_oaep_md:sha256",
            "-pkeyopt",
            "rsa_mgf1_md:sha256",
            "-pkeyopt",
            &format!("rsa_oaep_label:{label}"),
        ],
        wrapped,
        384,
    )
    .await
}

fn decode(s: &str, min: usize, max: usize) -> Result<Vec<u8>, Error> {
    if s.len() > max.div_ceil(3) * 4 {
        return Err(Error::Invalid);
    }
    let data = STANDARD.decode(s).map_err(|_| Error::Invalid)?;
    if !(min..=max).contains(&data.len()) {
        return Err(Error::Invalid);
    }
    Ok(data)
}
