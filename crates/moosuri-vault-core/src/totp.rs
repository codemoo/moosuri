use crate::{Error, Result, SecretText};
use data_encoding::BASE32_NOPAD;
use hmac::{Hmac, Mac};
use sha1::Sha1;
use sha2::{Sha256, Sha512};
use std::collections::HashSet;
use zeroize::Zeroizing;

#[derive(Clone, Copy)]
pub enum TotpAlgorithm {
    Sha1,
    Sha256,
    Sha512,
}
pub struct Totp {
    seed: Zeroizing<Vec<u8>>,
    algorithm: TotpAlgorithm,
    digits: u8,
    period: u16,
}
pub struct OtpCode {
    code: SecretText,
    pub valid_from: u64,
    pub valid_until: u64,
}
impl OtpCode {
    /// Trusted client display/typed adapter only. Never expose through a model tool.
    pub fn expose(&self) -> &str {
        self.code.expose()
    }
}
impl Totp {
    pub fn from_base32(
        seed: &SecretText,
        algorithm: TotpAlgorithm,
        digits: u8,
        period: u16,
    ) -> Result<Self> {
        if !(6..=8).contains(&digits) || !(15..=120).contains(&period) {
            return Err(Error::Unsupported);
        }
        if seed.expose().len() > 512 {
            return Err(Error::Limit);
        }
        // Permit common display spacing and lowercase, but no ignored arbitrary
        // punctuation or inconsistent padding that could hide a different seed.
        let mut text = Zeroizing::new(String::with_capacity(seed.expose().len()));
        for c in seed.expose().chars() {
            if c.is_ascii_whitespace() {
                continue;
            }
            if !(c.is_ascii_alphabetic() || ('2'..='7').contains(&c) || c == '=') {
                return Err(Error::Invalid);
            }
            text.push(c.to_ascii_uppercase());
        }
        let padding = text.len() - text.trim_end_matches('=').len();
        if text[..text.len() - padding].contains('=') {
            return Err(Error::Invalid);
        }
        if padding > 0 && (!text.len().is_multiple_of(8) || padding > 6) {
            return Err(Error::Invalid);
        }
        let unpadded = text.trim_end_matches('=');
        let bytes = Zeroizing::new(
            BASE32_NOPAD
                .decode(unpadded.as_bytes())
                .map_err(|_| Error::Invalid)?,
        );
        let canonical = Zeroizing::new(BASE32_NOPAD.encode(&bytes));
        if !(10..=128).contains(&bytes.len()) || canonical.as_str() != unpadded {
            return Err(Error::Invalid);
        }
        if padding > 0 && padding != (8 - unpadded.len() % 8) % 8 {
            return Err(Error::Invalid);
        }
        Ok(Self {
            seed: bytes,
            algorithm,
            digits,
            period,
        })
    }
    pub fn parse_uri(uri: &SecretText) -> Result<Self> {
        if uri.expose().len() > 4096 {
            return Err(Error::Limit);
        }
        let url = crate::secret::SecretUrl::parse(uri.expose())?;
        if url.scheme() != "otpauth" || url.host_str() != Some("totp") {
            return Err(Error::Unsupported);
        }
        if !url.username().is_empty()
            || url.password().is_some()
            || url.port().is_some()
            || url.fragment().is_some()
            || url.path().len() <= 1
        {
            return Err(Error::Invalid);
        }
        let mut seen = HashSet::new();
        let mut seed = None;
        let mut algorithm = TotpAlgorithm::Sha1;
        let mut digits = 6;
        let mut period = 30;
        for (k, v) in url.query_pairs() {
            let k = SecretText::new(k.into_owned())?;
            let v = SecretText::new(v.into_owned())?;
            let name = match k.expose() {
                "secret" => "secret",
                "issuer" => "issuer",
                "algorithm" => "algorithm",
                "digits" => "digits",
                "period" => "period",
                _ => return Err(Error::Unsupported),
            };
            if !seen.insert(name) {
                return Err(Error::Invalid);
            }
            match name {
                "secret" => seed = Some(v),
                "issuer" => {
                    if v.expose().len() > 512 {
                        return Err(Error::Limit);
                    }
                }
                "algorithm" => {
                    algorithm = match v.expose() {
                        "SHA1" => TotpAlgorithm::Sha1,
                        "SHA256" => TotpAlgorithm::Sha256,
                        "SHA512" => TotpAlgorithm::Sha512,
                        _ => return Err(Error::Unsupported),
                    }
                }
                "digits" => digits = v.expose().parse().map_err(|_| Error::Invalid)?,
                "period" => period = v.expose().parse().map_err(|_| Error::Invalid)?,
                _ => return Err(Error::Unsupported),
            }
        }
        Self::from_base32(&seed.ok_or(Error::Invalid)?, algorithm, digits, period)
    }
    pub fn code_at(&self, unix_seconds: u64) -> Result<OtpCode> {
        let period = u64::from(self.period);
        let counter = unix_seconds / period;
        let msg = counter.to_be_bytes();
        let digest = Zeroizing::new(match self.algorithm {
            TotpAlgorithm::Sha1 => {
                let mut h = Hmac::<Sha1>::new_from_slice(&self.seed).map_err(|_| Error::Invalid)?;
                h.update(&msg);
                h.finalize().into_bytes().to_vec()
            }
            TotpAlgorithm::Sha256 => {
                let mut h =
                    Hmac::<Sha256>::new_from_slice(&self.seed).map_err(|_| Error::Invalid)?;
                h.update(&msg);
                h.finalize().into_bytes().to_vec()
            }
            TotpAlgorithm::Sha512 => {
                let mut h =
                    Hmac::<Sha512>::new_from_slice(&self.seed).map_err(|_| Error::Invalid)?;
                h.update(&msg);
                h.finalize().into_bytes().to_vec()
            }
        });
        let offset = usize::from(digest[digest.len() - 1] & 15);
        let part: &[u8; 4] = digest[offset..offset + 4]
            .try_into()
            .map_err(|_| Error::Invalid)?;
        let number = (u32::from_be_bytes(*part) & 0x7fff_ffff) % 10u32.pow(u32::from(self.digits));
        let from = counter.checked_mul(period).ok_or(Error::Limit)?;
        let until = from.checked_add(period).ok_or(Error::Limit)?;
        Ok(OtpCode {
            code: SecretText::new(format!(
                "{number:0width$}",
                width = usize::from(self.digits)
            ))?,
            valid_from: from,
            valid_until: until,
        })
    }
}
