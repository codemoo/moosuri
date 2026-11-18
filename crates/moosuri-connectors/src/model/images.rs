//! Private in-memory media transport. Decoding and canonical re-encoding belong
//! to the server media loader; this boundary never accepts URLs or file paths.
use super::protocol::ProtocolError;

pub const MAX_IMAGES: usize = 4;
pub const MAX_IMAGE_BYTES: usize = 2 * 1024 * 1024;
pub const MAX_IMAGE_REQUEST_BYTES: usize = 12 * 1024 * 1024;

/// A bounded JPEG supplied by the trusted media normalization path.
/// Framing checks are not a decoder or a claim that untrusted uploads are safe.
/// No Debug/Serialize implementation: private bytes only enter an inference body.
#[derive(Clone)]
pub struct NormalizedJpeg(Vec<u8>);
impl NormalizedJpeg {
    pub fn new(bytes: Vec<u8>) -> Result<Self, ProtocolError> {
        if bytes.len() > MAX_IMAGE_BYTES {
            return Err(ProtocolError::LimitExceeded);
        }
        if bytes.len() < 5
            || !bytes.starts_with(&[0xff, 0xd8, 0xff])
            || !bytes.ends_with(&[0xff, 0xd9])
        {
            return Err(ProtocolError::Protocol);
        }
        Ok(Self(bytes))
    }
    pub fn bytes(&self) -> &[u8] {
        &self.0
    }
    pub fn byte_len(&self) -> usize {
        self.0.len()
    }
}
pub(crate) fn validate_count(images: &[NormalizedJpeg]) -> Result<(), ProtocolError> {
    if images.len() > MAX_IMAGES {
        return Err(ProtocolError::LimitExceeded);
    }
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn rejects_foreign_truncated_and_oversized_payloads() {
        for bytes in [
            vec![],
            b"https://foreign/image".to_vec(),
            b"/private/photo.jpg".to_vec(),
            vec![255, 216, 255],
            vec![255, 216, 255, 0],
        ] {
            assert!(matches!(
                NormalizedJpeg::new(bytes),
                Err(ProtocolError::Protocol)
            ));
        }
        assert!(matches!(
            NormalizedJpeg::new(vec![0; MAX_IMAGE_BYTES + 1]),
            Err(ProtocolError::LimitExceeded)
        ));
        let mut bytes = vec![0; MAX_IMAGE_BYTES];
        bytes[..3].copy_from_slice(&[255, 216, 255]);
        bytes[MAX_IMAGE_BYTES - 2..].copy_from_slice(&[255, 217]);
        let image = NormalizedJpeg::new(bytes).unwrap();
        assert_eq!(image.byte_len(), MAX_IMAGE_BYTES);
        assert!(validate_count(&vec![image.clone(); MAX_IMAGES]).is_ok());
        assert!(matches!(
            validate_count(&vec![image; MAX_IMAGES + 1]),
            Err(ProtocolError::LimitExceeded)
        ));
    }
}
