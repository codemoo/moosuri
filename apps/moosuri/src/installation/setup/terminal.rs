//! Controlling-terminal UI: pipes/log redirection never become a secret channel.
use super::super::Result;
use clap::ValueEnum;
use serde::{Deserialize, Serialize};
use std::{
    fs::{File, OpenOptions},
    io::{Read, Write},
    os::fd::AsRawFd,
};

#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, ValueEnum)]
#[serde(rename_all = "snake_case")]
pub enum Language {
    #[default]
    En,
    Ko,
}
impl Language {
    pub fn text<'a>(self, en: &'a str, ko: &'a str) -> &'a str {
        if matches!(self, Self::Ko) { ko } else { en }
    }
    pub fn detected() -> Self {
        let locale = std::env::var("LC_ALL")
            .ok()
            .filter(|s| !s.is_empty())
            .or_else(|| std::env::var("LC_MESSAGES").ok())
            .or_else(|| std::env::var("LANG").ok())
            .unwrap_or_default();
        if locale.to_lowercase().starts_with("ko") {
            Self::Ko
        } else {
            Self::En
        }
    }
}
pub struct Terminal {
    file: File,
    pub lang: Language,
    plain: bool,
}
impl Terminal {
    pub fn open(language: Option<Language>, plain: bool) -> Result<Self> {
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .open("/dev/tty")
            .map_err(|_| "setup_terminal_required".to_string())?;
        if unsafe { libc::isatty(file.as_raw_fd()) } != 1 {
            return Err("setup_terminal_required".into());
        }
        Ok(Self {
            file,
            lang: language.unwrap_or_else(Language::detected),
            plain: plain || std::env::var("TERM").is_ok_and(|v| v == "dumb"),
        })
    }
    pub fn say(&mut self, en: &str, ko: &str) -> Result<()> {
        let s = self.lang.text(en, ko);
        writeln!(self.file, "{s}").map_err(|_| "setup_terminal_io_failed".into())
    }
    pub fn line(&mut self, text: &str) -> Result<()> {
        writeln!(self.file, "{text}").map_err(|_| "setup_terminal_io_failed".into())
    }
    pub fn input(&mut self, en: &str, ko: &str, default: &str) -> Result<String> {
        loop {
            write!(
                self.file,
                "{}{}: ",
                self.lang.text(en, ko),
                if default.is_empty() {
                    String::new()
                } else {
                    format!(" [{default}]")
                }
            )
            .map_err(|_| "setup_terminal_io_failed")?;
            self.file.flush().map_err(|_| "setup_terminal_io_failed")?;
            let mut bytes = Vec::new();
            loop {
                let mut b = [0];
                self.file
                    .read_exact(&mut b)
                    .map_err(|_| "setup_cancelled")?;
                if b[0] == b'\n' {
                    break;
                }
                if bytes.len() >= 4096 {
                    return Err("setup_input_too_long".into());
                }
                bytes.push(b[0]);
            }
            let value = String::from_utf8(bytes).map_err(|_| "setup_invalid_text")?;
            let value = value.trim();
            if value == "q" {
                return Err("setup_cancelled".into());
            }
            if value.chars().any(char::is_control) {
                self.say("Please use plain text.", "일반 텍스트로 입력해주세요.")?;
                continue;
            }
            return Ok(if value.is_empty() {
                default.to_owned()
            } else {
                value.to_owned()
            });
        }
    }
    pub fn secret(&mut self, en: &str, ko: &str) -> Result<zeroize::Zeroizing<String>> {
        rpassword::prompt_password(format!("{}: ", self.lang.text(en, ko)))
            .map(zeroize::Zeroizing::new)
            .map_err(|_| "setup_password_input_failed".into())
    }
    pub fn select(
        &mut self,
        en: &str,
        ko: &str,
        items: &[(&str, &str)],
        default: usize,
    ) -> Result<usize> {
        self.say(en, ko)?;
        if !self.plain {
            return self.arrows(items, default);
        }
        for (i, (en, ko)) in items.iter().enumerate() {
            self.line(&format!(
                "  {}. {}{}",
                i + 1,
                self.lang.text(en, ko),
                if i == default {
                    self.lang.text(" (default)", " (기본값)")
                } else {
                    ""
                }
            ))?;
        }
        loop {
            let s = self.input(
                "Choose a number (Enter = default, q = quit)",
                "번호 선택 (Enter = 기본값, q = 종료)",
                &(default + 1).to_string(),
            )?;
            if let Ok(n) = s.parse::<usize>()
                && n > 0
                && n <= items.len()
            {
                return Ok(n - 1);
            }
            self.say(
                "Choose one of the listed numbers.",
                "목록에 있는 번호를 선택해주세요.",
            )?;
        }
    }
    fn arrows(&mut self, items: &[(&str, &str)], default: usize) -> Result<usize> {
        let mut mode = unsafe { std::mem::zeroed::<libc::termios>() };
        let fd = self.file.as_raw_fd();
        if unsafe { libc::tcgetattr(fd, &mut mode) } != 0 {
            return Err("setup_terminal_io_failed".into());
        }
        let mut raw = mode;
        raw.c_lflag &= !(libc::ICANON | libc::ECHO | libc::ISIG);
        raw.c_cc[libc::VMIN] = 1;
        raw.c_cc[libc::VTIME] = 0;
        if unsafe { libc::tcsetattr(fd, libc::TCSANOW, &raw) } != 0 {
            return Err("setup_terminal_io_failed".into());
        }
        struct Restore(i32, libc::termios);
        impl Drop for Restore {
            fn drop(&mut self) {
                unsafe {
                    libc::tcsetattr(self.0, libc::TCSANOW, &self.1);
                }
            }
        }
        let _restore = Restore(fd, mode);
        let mut selected = default;
        self.say(
            "  ↑/↓ or number · Enter to continue · q to quit",
            "  ↑/↓ 또는 번호 · Enter로 진행 · q로 종료",
        )?;
        loop {
            for (i, (en, ko)) in items.iter().enumerate() {
                writeln!(
                    self.file,
                    "\r\x1b[2K  {} {}. {}",
                    if i == selected { "›" } else { " " },
                    i + 1,
                    self.lang.text(en, ko)
                )
                .map_err(|_| "setup_terminal_io_failed")?;
            }
            self.file.flush().map_err(|_| "setup_terminal_io_failed")?;
            let mut b = [0];
            self.file
                .read_exact(&mut b)
                .map_err(|_| "setup_cancelled")?;
            match b[0] {
                b'\r' | b'\n' => return Ok(selected),
                3 | 4 | b'q' => return Err("setup_cancelled".into()),
                b'k' => selected = selected.saturating_sub(1),
                b'j' => selected = (selected + 1).min(items.len() - 1),
                b'1'..=b'9' => {
                    let n = (b[0] - b'1') as usize;
                    if n < items.len() {
                        selected = n;
                    }
                }
                27 => {
                    let mut poll = libc::pollfd {
                        fd,
                        events: libc::POLLIN,
                        revents: 0,
                    };
                    if unsafe { libc::poll(&mut poll, 1, 100) } > 0 {
                        self.file
                            .read_exact(&mut b)
                            .map_err(|_| "setup_cancelled")?;
                        if b[0] == b'[' && unsafe { libc::poll(&mut poll, 1, 100) } > 0 {
                            self.file
                                .read_exact(&mut b)
                                .map_err(|_| "setup_cancelled")?;
                            match b[0] {
                                b'A' => selected = selected.saturating_sub(1),
                                b'B' => selected = (selected + 1).min(items.len() - 1),
                                _ => (),
                            }
                        }
                    }
                }
                _ => (),
            }
            write!(self.file, "\x1b[{}A", items.len()).map_err(|_| "setup_terminal_io_failed")?;
        }
    }
}
