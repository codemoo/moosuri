//! Streaming local CSV import. No values or plaintext hashes in preview metadata.
use crate::{
    ActiveVault, Cancellation, EncryptedItem, Error, ItemFields, ItemKind, Result, SecretText,
    TrustedEntropy,
    crypto::random_id,
    secret::{SecretUrl, utf8},
    wire::MAX_ITEMS,
};
use sha2::{Digest, Sha256};
use std::{collections::HashSet, io::Read};
use zeroize::Zeroizing;

const CSV_LIMIT: usize = 10 * 1024 * 1024;
const ROW_LIMIT: usize = 65_536;
const COLUMN_LIMIT: usize = 16;
pub enum UsernameFilter {
    All,
    Exact(SecretText),
}
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum ImportStatus {
    New,
    ChangedAccount,
    Duplicate,
    Filtered,
}
pub struct ImportRow {
    pub row: u32,
    pub status: ImportStatus,
    pub blank_password: bool,
    pub unsupported_url: bool,
    pub invalid_otp: bool,
}
pub struct ImportPreview {
    pub rows: Vec<ImportRow>,
    pub empty_rows: u32,
    pub eligible: u32,
    pub duplicates: u32,
    pub filtered: u32,
}
struct Row {
    number: u32,
    fields: ItemFields,
}
pub struct AppleImport {
    header: [u8; 32],
    preview: ImportPreview,
    rows: Vec<Row>,
    batch: Option<(Vec<u32>, Vec<EncryptedItem>)>,
}
impl AppleImport {
    /// `existing` must be the trusted client's complete loaded item list when
    /// duplicate-against-existing counts are required; no server lookup occurs.
    pub fn preview(
        reader: impl Read,
        vault: &mut ActiveVault,
        existing: &[EncryptedItem],
        filter: UsernameFilter,
        cancel: &impl Cancellation,
    ) -> Result<Self> {
        if existing.len() > MAX_ITEMS {
            return Err(Error::Limit);
        }
        let mut contents = HashSet::new();
        let mut accounts = HashSet::new();
        for item in existing {
            if cancel.cancelled() {
                return Err(Error::Cancelled);
            }
            let fields = vault.open_item(item)?;
            contents.insert(fingerprint(&fields)?);
            accounts.insert(account_fingerprint(&fields)?);
        }
        let header = vault.header_digest()?;
        let mut csv = Csv::new(reader, cancel);
        let Some(raw_header) = csv.record()? else {
            return Err(Error::Csv);
        };
        let headers: Vec<SecretText> = raw_header.into_iter().map(utf8).collect::<Result<_>>()?;
        let mut names = Vec::with_capacity(headers.len());
        for h in headers {
            let name = Zeroizing::new(h.expose().trim().to_ascii_lowercase());
            let known = match name.as_str() {
                "title" => "title",
                "url" => "url",
                "username" => "username",
                "password" => "password",
                "notes" => "notes",
                "otpauth" => "otpauth",
                "favorite" => "favorite",
                _ => return Err(Error::Csv),
            };
            if names.contains(&known) {
                return Err(Error::Csv);
            }
            names.push(known);
        }
        for required in ["title", "url", "username", "password"] {
            if !names.contains(&required) {
                return Err(Error::Csv);
            }
        }
        let mut preview = ImportPreview {
            rows: vec![],
            empty_rows: 0,
            eligible: 0,
            duplicates: 0,
            filtered: 0,
        };
        let mut rows = Vec::new();
        let mut number = 1u32;
        while let Some(raw) = csv.record()? {
            number = number.checked_add(1).ok_or(Error::Limit)?;
            if number > 10_001 {
                return Err(Error::Limit);
            }
            if raw.iter().all(|v| v.is_empty()) {
                preview.empty_rows += 1;
                continue;
            }
            if raw.len() != names.len() {
                return Err(Error::Csv);
            }
            if preview.rows.len() >= MAX_ITEMS {
                return Err(Error::Limit);
            }
            let mut cells: Vec<Option<SecretText>> = raw
                .into_iter()
                .map(|b| utf8(b).map(Some))
                .collect::<Result<_>>()?;
            let mut take = |name: &str| -> Result<SecretText> {
                if let Some(i) = names.iter().position(|s| *s == name) {
                    cells[i].take().ok_or(Error::Csv)
                } else {
                    SecretText::new(String::new())
                }
            };
            let favorite = take("favorite")?;
            let favorite = match favorite.expose() {
                "" | "0" | "false" => false,
                "1" | "true" => true,
                _ => return Err(Error::Csv),
            };
            let fields = ItemFields {
                kind: ItemKind::Login,
                title: take("title")?,
                username: take("username")?,
                url: take("url")?,
                secret: take("password")?,
                notes: take("notes")?,
                favorite,
                expires_at: SecretText::new(String::new())?,
                totp: take("otpauth")?,
            };
            fields.encode()?;
            let selected = match &filter {
                UsernameFilter::All => true,
                UsernameFilter::Exact(value) => fields.username.expose() == value.expose(),
            };
            let blank_password = fields.secret.expose().is_empty();
            let unsupported_url = if fields.url.expose().is_empty() {
                false
            } else {
                match SecretUrl::parse(fields.url.expose()) {
                    Ok(u) => {
                        !matches!(u.scheme(), "http" | "https")
                            || !u.username().is_empty()
                            || u.password().is_some()
                    }
                    Err(_) => true,
                }
            };
            let invalid_otp =
                !fields.totp.expose().is_empty() && crate::Totp::parse_uri(&fields.totp).is_err();
            let content = fingerprint(&fields)?;
            let identity = account_fingerprint(&fields)?;
            let status = if !selected {
                preview.filtered += 1;
                ImportStatus::Filtered
            } else if contents.contains(&content) {
                preview.duplicates += 1;
                ImportStatus::Duplicate
            } else {
                preview.eligible += 1;
                if accounts.contains(&identity) {
                    ImportStatus::ChangedAccount
                } else {
                    ImportStatus::New
                }
            };
            preview.rows.push(ImportRow {
                row: number,
                status,
                blank_password,
                unsupported_url,
                invalid_otp,
            });
            if selected {
                contents.insert(content);
                accounts.insert(identity);
            }
            if matches!(status, ImportStatus::New | ImportStatus::ChangedAccount) {
                rows.push(Row { number, fields });
            }
        }
        if preview.rows.is_empty() {
            return Err(Error::Csv);
        }
        Ok(Self {
            header,
            preview,
            rows,
            batch: None,
        })
    }
    pub fn counts(&self) -> &ImportPreview {
        &self.preview
    }
    /// Stable encrypted batch on retry. Caller commits only the chosen immutable
    /// ciphertexts and tracks partial ACKs; this function makes no storage claim.
    pub fn seal_selected<'a>(
        &'a mut self,
        vault: &mut ActiveVault,
        selected: &[u32],
        entropy: &mut impl TrustedEntropy,
        cancel: &impl Cancellation,
    ) -> Result<&'a [EncryptedItem]> {
        if self.header != vault.header_digest()? {
            return Err(Error::Conflict);
        }
        if selected.is_empty() || selected.len() > MAX_ITEMS {
            return Err(Error::Invalid);
        }
        if cancel.cancelled() {
            return Err(Error::Cancelled);
        }
        if self
            .batch
            .as_ref()
            .is_some_and(|(prior, _)| prior != selected)
        {
            return Err(Error::Conflict);
        }
        if self.batch.is_none() {
            let mut unique = HashSet::new();
            for n in selected {
                if !unique.insert(*n) || !self.rows.iter().any(|r| r.number == *n) {
                    return Err(Error::Invalid);
                }
            }
            let mut batch = Vec::with_capacity(selected.len());
            for number in selected {
                if cancel.cancelled() {
                    return Err(Error::Cancelled);
                }
                let row = self
                    .rows
                    .iter()
                    .find(|r| r.number == *number)
                    .ok_or(Error::Invalid)?;
                batch.push(vault.seal_item(random_id(entropy)?, 1, &row.fields, entropy)?);
            }
            self.batch = Some((selected.to_vec(), batch));
            self.rows.clear();
        }
        Ok(&self.batch.as_ref().ok_or(Error::State)?.1)
    }
}
fn fingerprint(fields: &ItemFields) -> Result<[u8; 32]> {
    Ok(Sha256::digest(&*fields.encode()?).into())
}
fn account_fingerprint(fields: &ItemFields) -> Result<[u8; 32]> {
    let mut hash = Sha256::new();
    hash.update(b"moosuri-vault/v2/import-identity\0");
    hash.update(match fields.kind {
        ItemKind::Login => b"login".as_slice(),
        ItemKind::ApiKey => b"api_key".as_slice(),
        ItemKind::Note => b"note".as_slice(),
    });
    let site = match SecretUrl::parse(fields.url.expose()) {
        Ok(url) if matches!(url.scheme(), "http" | "https") => {
            Zeroizing::new(url.origin().ascii_serialization())
        }
        _ => Zeroizing::new(fields.url.expose().to_owned()),
    };
    for text in [
        site.as_str(),
        fields.username.expose(),
        if site.is_empty() && fields.username.expose().is_empty() {
            fields.title.expose()
        } else {
            ""
        },
    ] {
        hash.update((text.len() as u64).to_be_bytes());
        hash.update(text.as_bytes());
    }
    Ok(hash.finalize().into())
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum State {
    Start,
    Plain,
    Quoted,
    Closed,
}
struct Csv<'a, R, C> {
    reader: R,
    cancel: &'a C,
    buffer: [u8; 8192],
    at: usize,
    end: usize,
    total: usize,
    prefix: Vec<u8>,
    skip_lf: bool,
    eof: bool,
}
impl<'a, R: Read, C: Cancellation> Csv<'a, R, C> {
    fn new(reader: R, cancel: &'a C) -> Self {
        Self {
            reader,
            cancel,
            buffer: [0; 8192],
            at: 0,
            end: 0,
            total: 0,
            prefix: vec![],
            skip_lf: false,
            eof: false,
        }
    }
    fn byte(&mut self) -> Result<Option<u8>> {
        if self.cancel.cancelled() {
            return Err(Error::Cancelled);
        }
        if let Some(b) = self.prefix.pop() {
            return Ok(Some(b));
        }
        if self.at == self.end {
            self.buffer.fill(0);
            self.end = self.reader.read(&mut self.buffer).map_err(|_| Error::Csv)?;
            self.at = 0;
            if self.end == 0 {
                return Ok(None);
            }
        }
        let b = self.buffer[self.at];
        self.buffer[self.at] = 0;
        self.at += 1;
        self.total = self.total.checked_add(1).ok_or(Error::Limit)?;
        if self.total > CSV_LIMIT {
            return Err(Error::Limit);
        }
        Ok(Some(b))
    }
    fn record(&mut self) -> Result<Option<Vec<Zeroizing<Vec<u8>>>>> {
        if self.eof {
            return Ok(None);
        }
        if self.total == 0 && self.prefix.is_empty() {
            let mut prefix = Vec::with_capacity(3);
            for _ in 0..3 {
                if let Some(b) = self.byte()? {
                    prefix.push(b)
                } else {
                    break;
                }
            }
            if prefix != [0xef, 0xbb, 0xbf] {
                self.prefix = prefix.into_iter().rev().collect();
            }
        }
        let mut state = State::Start;
        let mut row = Vec::new();
        let mut cell = Zeroizing::new(Vec::new());
        let mut row_bytes = 0usize;
        let mut any = false;
        loop {
            let next = self.byte()?;
            if self.skip_lf {
                self.skip_lf = false;
                if next == Some(b'\n') {
                    continue;
                }
            }
            let Some(b) = next else {
                self.eof = true;
                if state == State::Quoted {
                    return Err(Error::Csv);
                }
                if !any {
                    return Ok(None);
                }
                row.push(cell);
                return Ok(Some(row));
            };
            any = true;
            if b == 0 {
                return Err(Error::Csv);
            }
            if state != State::Quoted && matches!(b, b'\r' | b'\n') {
                if b == b'\r' {
                    self.skip_lf = true
                }
                row.push(cell);
                return Ok(Some(row));
            }
            match (state, b) {
                (State::Start, b'"') => state = State::Quoted,
                (State::Quoted, b'"') => state = State::Closed,
                (State::Closed, b'"') => {
                    cell.push(b'"');
                    row_bytes += 1;
                    state = State::Quoted
                }
                (State::Start | State::Plain | State::Closed, b',') => {
                    row.push(cell);
                    if row.len() >= COLUMN_LIMIT {
                        return Err(Error::Limit);
                    }
                    cell = Zeroizing::new(Vec::new());
                    state = State::Start
                }
                (State::Closed, _) | (State::Plain, b'"') => return Err(Error::Csv),
                (_, b) => {
                    cell.push(b);
                    row_bytes += 1;
                    if state == State::Start {
                        state = State::Plain
                    }
                }
            }
            if row_bytes > ROW_LIMIT || cell.len() > ROW_LIMIT {
                return Err(Error::Limit);
            }
        }
    }
}
impl<R, C> Drop for Csv<'_, R, C> {
    fn drop(&mut self) {
        use zeroize::Zeroize;
        self.buffer.zeroize();
        self.prefix.zeroize();
    }
}
