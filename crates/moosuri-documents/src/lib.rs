//! Text-only PDF extraction. The parser runs in an isolated, resource-limited child.
#![deny(unsafe_code)]

use serde::{Deserialize, Serialize};
use std::{
    io::{Read, Write},
    path::Path,
    process::Stdio,
    time::Duration,
};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

pub const MAX_PDF_BYTES: usize = 2 * 1024 * 1024;
pub const MAX_TEXT_BYTES: usize = 24 * 1024;
const MAX_RESULT_BYTES: usize = 64 * 1024;
const MAX_PAGES: usize = 50;
const MAX_OBJECTS: usize = 20_000;
const MAX_DECOMPRESSED_BYTES: usize = 32 * 1024 * 1024;
pub const CHILD_ARGUMENT: &str = "internal-pdf-text";

mod preflight;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PdfError {
    Invalid,
    Encrypted,
    NoText,
    Unsupported,
    TooLarge,
    Unavailable,
    TimedOut,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PdfText {
    pub pages: u32,
    pub text: String,
}

impl PdfText {
    pub fn validate(&self) -> Result<(), PdfError> {
        if self.pages == 0 || self.pages as usize > MAX_PAGES || self.text.len() > MAX_TEXT_BYTES {
            return Err(PdfError::TooLarge);
        }
        if self.text.trim().is_empty() {
            return Err(PdfError::NoText);
        }
        if self
            .text
            .chars()
            .any(|c| c.is_control() && !matches!(c, '\n' | '\r' | '\t'))
        {
            return Err(PdfError::Invalid);
        }
        Ok(())
    }
}

/// Only call in the resource-limited child (unit tests use small synthetic documents).
fn parse_pdf(input: &[u8]) -> Result<PdfText, PdfError> {
    if input.is_empty() || input.len() > MAX_PDF_BYTES {
        return Err(PdfError::TooLarge);
    }
    if !input.starts_with(b"%PDF-") {
        return Err(PdfError::Invalid);
    }
    let doc = lopdf::Document::load_mem_with_options(
        input,
        lopdf::LoadOptions {
            strict: true,
            max_decompressed_size: Some(MAX_DECOMPRESSED_BYTES),
            ..Default::default()
        },
    )
    .map_err(parser_error)?;
    if doc.is_encrypted() || doc.was_encrypted() {
        return Err(PdfError::Encrypted);
    }
    if doc.objects.len() > MAX_OBJECTS {
        return Err(PdfError::TooLarge);
    }
    let pages = doc.get_pages();
    if pages.is_empty() || pages.len() > MAX_PAGES {
        return Err(PdfError::TooLarge);
    }
    preflight::document(&doc, &pages)?;
    let mut text = String::new();
    let mut has_text = false;
    for (number, page_id) in &pages {
        preflight::page(&doc, *page_id)?;
        let page = doc
            .extract_text_with_limit(&[*number], MAX_DECOMPRESSED_BYTES)
            .map_err(parser_error)?;
        if !page.trim().is_empty() {
            has_text = true;
        }
        let heading = format!("[Page {number}]\n");
        if text
            .len()
            .saturating_add(heading.len())
            .saturating_add(page.len())
            .saturating_add(1)
            > MAX_TEXT_BYTES
        {
            return Err(PdfError::TooLarge);
        }
        text.push_str(&heading);
        text.push_str(&page);
        text.push('\n');
    }
    if !has_text {
        return Err(PdfError::NoText);
    }
    let result = PdfText {
        pages: pages.len() as u32,
        text,
    };
    result.validate()?;
    Ok(result)
}

fn parser_error(error: lopdf::Error) -> PdfError {
    match error {
        lopdf::Error::Decompress(lopdf::DecompressError::MemoryLimitExceeded { .. }) => {
            PdfError::TooLarge
        }
        lopdf::Error::InvalidPassword
        | lopdf::Error::Decryption(_)
        | lopdf::Error::UnsupportedSecurityHandler(_) => PdfError::Encrypted,
        lopdf::Error::Unimplemented(_) | lopdf::Error::CharacterEncoding => PdfError::Unsupported,
        _ => PdfError::Invalid,
    }
}

/// Hidden fixed CLI entry point. No file paths, passwords, network or installation state.
pub fn child_main() -> i32 {
    let mut input = Vec::new();
    let result = if std::io::stdin()
        .take((MAX_PDF_BYTES + 1) as u64)
        .read_to_end(&mut input)
        .is_err()
    {
        Err(PdfError::Invalid)
    } else {
        // Parser panic is a fixed failed extraction; its diagnostics never reach the parent.
        std::panic::set_hook(Box::new(|_| {}));
        std::panic::catch_unwind(|| parse_pdf(&input)).unwrap_or(Err(PdfError::Invalid))
    };
    match serde_json::to_vec(&result) {
        Ok(bytes) if bytes.len() <= MAX_RESULT_BYTES => {
            if std::io::stdout().write_all(&bytes).is_ok() {
                0
            } else {
                1
            }
        }
        _ => 1,
    }
}

#[cfg(unix)]
#[allow(unsafe_code)]
fn limits(command: &mut tokio::process::Command) {
    use std::os::unix::process::CommandExt;
    // SAFETY: the child-only pre-exec closure calls async-signal-safe setrlimit;
    // it does not allocate, acquire Rust locks, or touch parent process limits.
    unsafe {
        command.as_std_mut().pre_exec(|| {
            let limits = [
                (libc::RLIMIT_CPU, 5 as libc::rlim_t),
                (libc::RLIMIT_CORE, 0 as libc::rlim_t),
                (libc::RLIMIT_FSIZE, 0 as libc::rlim_t),
                (libc::RLIMIT_NOFILE, 32 as libc::rlim_t),
            ];
            for (resource, value) in limits {
                let limit = libc::rlimit {
                    rlim_cur: value,
                    rlim_max: value,
                };
                if libc::setrlimit(resource, &limit) != 0 {
                    return Err(std::io::Error::last_os_error());
                }
            }
            // Linux is the production deployment target. macOS reserves a large
            // executable address space; CPU/output/wall bounds still apply there.
            #[cfg(target_os = "linux")]
            {
                let bytes = 512 * 1024 * 1024;
                let limit = libc::rlimit {
                    rlim_cur: bytes,
                    rlim_max: bytes,
                };
                if libc::setrlimit(libc::RLIMIT_AS, &limit) != 0 {
                    return Err(std::io::Error::last_os_error());
                }
            }
            Ok(())
        });
    }
}

/// The executable is deployment-owned, never a request or model-selected path.
pub async fn extract_pdf(executable: &Path, input: &[u8]) -> Result<PdfText, PdfError> {
    if input.is_empty() || input.len() > MAX_PDF_BYTES {
        return Err(PdfError::TooLarge);
    }
    if !executable.is_absolute() {
        return Err(PdfError::Unavailable);
    }
    let mut command = tokio::process::Command::new(executable);
    command
        .arg(CHILD_ARGUMENT)
        .env_clear()
        .current_dir("/")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .kill_on_drop(true);
    #[cfg(unix)]
    limits(&mut command);
    let mut child = command.spawn().map_err(|_| PdfError::Unavailable)?;
    let mut stdin = child.stdin.take().ok_or(PdfError::Unavailable)?;
    let stdout = child.stdout.take().ok_or(PdfError::Unavailable)?;
    let work = async {
        let write = async move {
            stdin.write_all(input).await?;
            stdin.shutdown().await?;
            // Tokio's Unix shutdown does not close ChildStdin. The child reads
            // through EOF, so ownership must end before waiting for its output.
            drop(stdin);
            Ok::<(), std::io::Error>(())
        };
        let read = async {
            let mut result = Vec::new();
            stdout
                .take((MAX_RESULT_BYTES + 1) as u64)
                .read_to_end(&mut result)
                .await?;
            if result.len() > MAX_RESULT_BYTES {
                return Err(std::io::Error::other("pdf_result_limit"));
            }
            Ok(result)
        };
        tokio::try_join!(write, read, child.wait())
    };
    let response = tokio::time::timeout(Duration::from_secs(10), work).await;
    let (_, bytes, status) = match response {
        Ok(Ok(value)) => value,
        _ => {
            let _ = child.kill().await;
            return Err(if response.is_err() {
                PdfError::TimedOut
            } else {
                PdfError::Invalid
            });
        }
    };
    if !status.success() {
        return Err(PdfError::Invalid);
    }
    let parsed: Result<PdfText, PdfError> =
        serde_json::from_slice(&bytes).map_err(|_| PdfError::Invalid)?;
    let parsed = parsed?;
    parsed.validate()?;
    Ok(parsed)
}

#[cfg(test)]
mod tests;

#[cfg(all(test, unix))]
mod process_tests;
