//! Fixed OpenAI Audio API. Never accepts plan credentials or a caller URL.
use reqwest::{Client, multipart};
use std::time::Duration;
use tokio_util::sync::CancellationToken;
use zeroize::Zeroizing;
pub const MAX_WAV: usize = 44 + 120 * 16000 * 2;
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum AudioError {
    #[error("invalid_audio")]
    Invalid,
    #[error("audio_unavailable")]
    Unavailable,
    #[error("audio_credentials_rejected")]
    Credentials,
    #[error("audio_rate_limited")]
    RateLimited,
    #[error("audio_outcome_unknown")]
    Unknown,
    #[error("audio_response_invalid")]
    Response,
}
/// Closed canonical envelope; sample count, not client metadata, bounds duration.
pub fn validate_wav(bytes: &[u8]) -> Result<u32, AudioError> {
    if bytes.len() < 46
        || bytes.len() > MAX_WAV
        || &bytes[0..4] != b"RIFF"
        || &bytes[8..16] != b"WAVEfmt "
        || &bytes[36..40] != b"data"
    {
        return Err(AudioError::Invalid);
    }
    let u16at = |i| u16::from_le_bytes(bytes[i..i + 2].try_into().unwrap());
    let u32at = |i| u32::from_le_bytes(bytes[i..i + 4].try_into().unwrap());
    if u32at(4) as usize != bytes.len() - 8
        || u32at(16) != 16
        || u16at(20) != 1
        || u16at(22) != 1
        || u32at(24) != 16000
        || u32at(28) != 32000
        || u16at(32) != 2
        || u16at(34) != 16
        || u32at(40) as usize != bytes.len() - 44
        || (bytes.len() - 44) % 2 != 0
    {
        return Err(AudioError::Invalid);
    }
    Ok(((bytes.len() - 44) as u32).div_ceil(32))
}
#[derive(Clone, Default)]
pub struct AudioClient {
    #[cfg(feature = "test-fixtures")]
    fixture: Option<String>,
}
impl AudioClient {
    /// Only available in explicitly enabled synthetic acceptance builds.
    #[cfg(feature = "test-fixtures")]
    pub fn fixture(base: String) -> Result<Self, AudioError> {
        let u = url::Url::parse(&base).map_err(|_| AudioError::Invalid)?;
        if u.scheme() != "http"
            || u.host_str() != Some("127.0.0.1")
            || u.path() != "/"
            || u.query().is_some()
            || u.fragment().is_some()
            || !u.username().is_empty()
            || u.password().is_some()
        {
            return Err(AudioError::Invalid);
        }
        Ok(Self {
            fixture: Some(base),
        })
    }
    pub async fn transcribe(
        &self,
        secret: Zeroizing<String>,
        model: &str,
        wav: Vec<u8>,
        cancel: &CancellationToken,
    ) -> Result<String, AudioError> {
        validate_wav(&wav)?;
        if model != "gpt-4o-mini-transcribe"
            || secret.is_empty()
            || secret
                .bytes()
                .any(|b| b.is_ascii_whitespace() || b.is_ascii_control())
        {
            return Err(AudioError::Invalid);
        }
        let endpoint = "https://api.openai.com/v1/audio/transcriptions".to_owned();
        #[cfg(feature = "test-fixtures")]
        let endpoint = self
            .fixture
            .as_ref()
            .map(|b| format!("{b}v1/audio/transcriptions"))
            .unwrap_or(endpoint);
        let url = url::Url::parse(&endpoint).map_err(|_| AudioError::Unavailable)?;
        let host = url.host_str().ok_or(AudioError::Unavailable)?;
        let addresses: Vec<_> = tokio::net::lookup_host((
            host,
            url.port_or_known_default().ok_or(AudioError::Unavailable)?,
        ))
        .await
        .map_err(|_| AudioError::Unavailable)?
        .take(33)
        .collect();
        #[cfg(feature = "test-fixtures")]
        let fixture = self.fixture.is_some();
        #[cfg(not(feature = "test-fixtures"))]
        let fixture = false;
        if addresses.is_empty()
            || addresses.len() > 32
            || addresses.iter().any(|a| {
                !(super::model::public_audio_address(a.ip()) || fixture && a.ip().is_loopback())
            })
        {
            return Err(AudioError::Unavailable);
        }
        let client = Client::builder()
            .no_proxy()
            .redirect(reqwest::redirect::Policy::none())
            .retry(reqwest::retry::never())
            .connect_timeout(Duration::from_secs(10))
            .read_timeout(Duration::from_secs(30))
            .timeout(Duration::from_secs(60))
            .resolve_to_addrs(host, &addresses)
            .build()
            .map_err(|_| AudioError::Unavailable)?;
        let part = multipart::Part::bytes(wav)
            .file_name("dictation.wav")
            .mime_str("audio/wav")
            .map_err(|_| AudioError::Invalid)?;
        let form = multipart::Form::new()
            .text("model", model.to_owned())
            .text("response_format", "json")
            .part("file", part);
        let operation = async {
            let mut response = client
                .post(url)
                .bearer_auth(secret.as_str())
                .multipart(form)
                .send()
                .await
                .map_err(|_| AudioError::Unknown)?;
            if !response.status().is_success() {
                return Err(match response.status().as_u16() {
                    401 | 403 => AudioError::Credentials,
                    429 => AudioError::RateLimited,
                    _ => AudioError::Unknown,
                });
            }
            if response
                .headers()
                .get(reqwest::header::CONTENT_TYPE)
                .and_then(|v| v.to_str().ok())
                .is_none_or(|v| v.split(';').next() != Some("application/json"))
            {
                return Err(AudioError::Response);
            }
            let mut body = Zeroizing::new(Vec::new());
            while let Some(chunk) = response.chunk().await.map_err(|_| AudioError::Unknown)? {
                if body.len() + chunk.len() > 65536 {
                    return Err(AudioError::Response);
                }
                body.extend_from_slice(&chunk);
            }
            let value: serde_json::Value =
                serde_json::from_slice(&body).map_err(|_| AudioError::Response)?;
            let text = value
                .get("text")
                .and_then(|v| v.as_str())
                .ok_or(AudioError::Response)?
                .trim();
            if text.is_empty()
                || text.len() > 24576
                || text
                    .chars()
                    .any(|c| c.is_control() && !matches!(c, '\n' | '\r' | '\t'))
            {
                return Err(AudioError::Response);
            }
            Ok(text.to_owned())
        };
        tokio::select! {_ = cancel.cancelled()=>Err(AudioError::Unknown), r=tokio::time::timeout(Duration::from_secs(60),operation)=>r.unwrap_or(Err(AudioError::Unknown))}
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    pub fn wav(samples: usize) -> Vec<u8> {
        let mut b = Vec::new();
        b.extend(b"RIFF");
        b.extend(((36 + samples * 2) as u32).to_le_bytes());
        b.extend(b"WAVEfmt ");
        b.extend(16u32.to_le_bytes());
        b.extend(1u16.to_le_bytes());
        b.extend(1u16.to_le_bytes());
        b.extend(16000u32.to_le_bytes());
        b.extend(32000u32.to_le_bytes());
        b.extend(2u16.to_le_bytes());
        b.extend(16u16.to_le_bytes());
        b.extend(b"data");
        b.extend(((samples * 2) as u32).to_le_bytes());
        b.resize(44 + samples * 2, 0);
        b
    }
    #[test]
    fn duration_comes_from_complete_pcm() {
        let b = wav(16000);
        assert_eq!(validate_wav(&b), Ok(1000));
        for i in [4, 16, 20, 22, 24, 28, 32, 34, 40] {
            let mut bad = b.clone();
            bad[i] ^= 1;
            assert_eq!(validate_wav(&bad), Err(AudioError::Invalid));
        }
        let mut extra = b;
        extra.push(0);
        assert!(validate_wav(&extra).is_err());
        assert!(validate_wav(&wav(120 * 16000 + 1)).is_err());
    }
}
