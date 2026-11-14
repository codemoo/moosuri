//! Closed canonical PCM only. No codec discovery, resampling or external process.
use crate::{audio::validate_wav, work_sessions::Result};
pub(crate) const MAX_CALL_WAV: usize = 44 + 1200 * 16000 * 2;
pub(crate) struct Chunk {
    pub bytes: Vec<u8>,
    pub duration_ms: u32,
}
pub(crate) fn chunks(bytes: &[u8]) -> Result<Vec<Chunk>> {
    if bytes.len() < 46 || bytes.len() > MAX_CALL_WAV || (bytes.len() - 44) % 2 != 0 {
        return Err("unsupported_call_wav");
    }
    let u32at = |i| u32::from_le_bytes(bytes[i..i + 4].try_into().unwrap()) as usize;
    if u32at(4) != bytes.len() - 8 || u32at(40) != bytes.len() - 44 {
        return Err("unsupported_call_wav");
    }
    let mut out = vec![];
    for data in bytes[44..].chunks(120 * 16000 * 2) {
        let mut wav = bytes[..44].to_vec();
        wav[4..8].copy_from_slice(&((36 + data.len()) as u32).to_le_bytes());
        wav[40..44].copy_from_slice(&(data.len() as u32).to_le_bytes());
        wav.extend_from_slice(data);
        let duration_ms = validate_wav(&wav).map_err(|_| "unsupported_call_wav")?;
        out.push(Chunk {
            bytes: wav,
            duration_ms,
        });
    }
    Ok(out)
}
