//! Reject silent loss in lopdf's deliberately lenient text extractor.
use super::{MAX_DECOMPRESSED_BYTES, MAX_PAGES, PdfError, parser_error};
use lopdf::{Dictionary, Document, Encoding, Object, ObjectId, Stream, content::Content};
use std::collections::{BTreeMap, HashSet};

pub(super) fn document(doc: &Document, pages: &BTreeMap<u32, ObjectId>) -> Result<(), PdfError> {
    fn walk(
        doc: &Document,
        id: ObjectId,
        depth: usize,
        seen: &mut HashSet<ObjectId>,
        leaves: &mut Vec<ObjectId>,
    ) -> Result<usize, PdfError> {
        if depth > 32 || !seen.insert(id) {
            return Err(PdfError::Invalid);
        }
        let node = doc.get_dictionary(id).map_err(parser_error)?;
        match node
            .get(b"Type")
            .and_then(Object::as_name)
            .map_err(parser_error)?
        {
            b"Page" => {
                leaves.push(id);
                if leaves.len() > MAX_PAGES {
                    return Err(PdfError::TooLarge);
                }
                Ok(1)
            }
            b"Pages" => {
                let kids = node
                    .get(b"Kids")
                    .and_then(Object::as_array)
                    .map_err(parser_error)?;
                let mut count = 0;
                for child in kids {
                    count += walk(
                        doc,
                        child.as_reference().map_err(parser_error)?,
                        depth + 1,
                        seen,
                        leaves,
                    )?;
                }
                if node
                    .get(b"Count")
                    .and_then(Object::as_i64)
                    .map_err(parser_error)?
                    != count as i64
                {
                    return Err(PdfError::Invalid);
                }
                Ok(count)
            }
            _ => Err(PdfError::Invalid),
        }
    }
    let root = doc
        .catalog()
        .map_err(parser_error)?
        .get(b"Pages")
        .and_then(Object::as_reference)
        .map_err(parser_error)?;
    let mut leaves = Vec::new();
    walk(doc, root, 0, &mut HashSet::new(), &mut leaves)?;
    if !leaves.iter().eq(pages.values()) {
        return Err(PdfError::Invalid);
    }
    Ok(())
}

fn resources(doc: &Document, mut page: ObjectId) -> Result<Option<&Dictionary>, PdfError> {
    let mut seen = HashSet::new();
    loop {
        if seen.len() >= 32 || !seen.insert(page) {
            return Err(PdfError::Invalid);
        }
        let node = doc.get_dictionary(page).map_err(parser_error)?;
        if let Ok(value) = node.get(b"Resources") {
            return doc
                .dereference(value)
                .and_then(|(_, value)| value.as_dict())
                .map(Some)
                .map_err(parser_error);
        }
        match node.get(b"Parent") {
            Ok(parent) => page = parent.as_reference().map_err(parser_error)?,
            Err(_) => return Ok(None),
        }
    }
}

fn resource<'a>(
    doc: &'a Document,
    resources: Option<&'a Dictionary>,
    group: &[u8],
    name: &[u8],
) -> Result<&'a Object, PdfError> {
    let resources = resources.ok_or(PdfError::Invalid)?;
    let group = doc
        .get_dict_in_dict(resources, group)
        .map_err(parser_error)?;
    let value = group.get(name).map_err(parser_error)?;
    doc.dereference(value)
        .map(|(_, value)| value)
        .map_err(parser_error)
}

fn stream_bytes(stream: &Stream, remaining: usize) -> Result<Vec<u8>, PdfError> {
    // The library treats a malformed Filter as an absent Filter. Never accept
    // compressed source as if it were already decoded text.
    if stream.dict.has(b"Filter") {
        stream.filters().map_err(parser_error)?;
    }
    stream
        .decompressed_content_with_limit(remaining)
        .map_err(parser_error)
}

fn contents(doc: &Document, page: ObjectId) -> Result<Vec<u8>, PdfError> {
    let page = doc.get_dictionary(page).map_err(parser_error)?;
    let Ok(value) = page.get(b"Contents") else {
        return Ok(Vec::new());
    };
    let (_, resolved) = doc.dereference(value).map_err(parser_error)?;
    let ids = match resolved {
        Object::Null => return Ok(Vec::new()),
        Object::Stream(_) => vec![value.as_reference().map_err(parser_error)?],
        Object::Array(values) => values
            .iter()
            .map(Object::as_reference)
            .collect::<Result<Vec<_>, _>>()
            .map_err(parser_error)?,
        _ => return Err(PdfError::Invalid),
    };
    let mut bytes = Vec::new();
    for id in ids {
        let stream = doc
            .get_object(id)
            .and_then(Object::as_stream)
            .map_err(parser_error)?;
        let remaining = MAX_DECOMPRESSED_BYTES.saturating_sub(bytes.len());
        let part = stream_bytes(stream, remaining)?;
        if part.len() >= remaining {
            return Err(PdfError::TooLarge);
        }
        bytes.extend_from_slice(&part);
        bytes.push(b'\n');
    }
    Ok(bytes)
}

fn font<'a>(doc: &'a Document, font: &'a Dictionary) -> Result<Encoding<'a>, PdfError> {
    if font
        .get(b"Subtype")
        .and_then(Object::as_name)
        .map_err(parser_error)?
        == b"Type3"
    {
        return Err(PdfError::Unsupported);
    }
    if let Ok(map) = font.get(b"ToUnicode") {
        let map = doc
            .dereference(map)
            .and_then(|(_, v)| v.as_stream())
            .map_err(parser_error)?;
        stream_bytes(map, MAX_DECOMPRESSED_BYTES)?;
    }
    let encoding = font
        .get_font_encoding_with_limit(doc, MAX_DECOMPRESSED_BYTES)
        .map_err(parser_error)?;
    // Invalid or unimplemented maps silently fall back to StandardEncoding.
    // Require the declared mapping to be the mapping actually used.
    if font.has(b"ToUnicode") && !matches!(encoding, Encoding::UnicodeMapEncoding(_)) {
        return Err(PdfError::Unsupported);
    }
    if let Ok(value) = font.get(b"Encoding") {
        let (_, value) = doc.dereference(value).map_err(parser_error)?;
        match value {
            Object::Name(name) if matches!(name.as_slice(), b"Identity-H" | b"Identity-V") => {
                if !matches!(encoding, Encoding::UnicodeMapEncoding(_)) {
                    return Err(PdfError::Unsupported);
                }
            }
            Object::Dictionary(dict) if dict.has(b"Differences") => {
                if !matches!(encoding, Encoding::Differences(_)) {
                    return Err(PdfError::Unsupported);
                }
            }
            Object::Name(_) | Object::Dictionary(_) => {}
            _ => return Err(PdfError::Invalid),
        }
    }
    Ok(encoding)
}

fn number(value: &Object) -> bool {
    matches!(value, Object::Integer(_) | Object::Real(_))
}

fn text_bytes(encoding: &Encoding<'_>, value: &Object) -> Result<(), PdfError> {
    let bytes = value.as_str().map_err(parser_error)?;
    if let Encoding::OneByteEncoding(map) = encoding
        && bytes.iter().any(|byte| map[*byte as usize].is_none())
    {
        return Err(PdfError::Unsupported);
    }
    let decoded = encoding.bytes_to_string(bytes).map_err(parser_error)?;
    if (!bytes.is_empty() && decoded.is_empty()) || decoded.contains('\u{fffd}') {
        return Err(PdfError::Unsupported);
    }
    Ok(())
}

pub(super) fn page(doc: &Document, page: ObjectId) -> Result<(), PdfError> {
    let resources = resources(doc, page)?;
    let bytes = contents(doc, page)?;
    let content = Content::decode_strict(&bytes).map_err(parser_error)?;
    if content.operations.len() > 50_000 {
        return Err(PdfError::TooLarge);
    }
    let fonts = doc.get_page_fonts(page).map_err(parser_error)?;
    let encodings = fonts
        .iter()
        .map(|(name, value)| font(doc, value).map(|encoding| (name.as_slice(), encoding)))
        .collect::<Result<BTreeMap<_, _>, _>>()?;
    let mut current_font: Option<&[u8]> = None;
    let mut extractor_font: Option<&[u8]> = None;
    let mut stack = Vec::new();
    let mut in_text = false;
    for op in &content.operations {
        let args = op.operands.as_slice();
        match op.operator.as_str() {
            "BT" if args.is_empty() && !in_text => in_text = true,
            "ET" if args.is_empty() && in_text => in_text = false,
            "BT" | "ET" => return Err(PdfError::Invalid),
            "q" if args.is_empty() && stack.len() < 32 => stack.push(current_font),
            "Q" if args.is_empty() => current_font = stack.pop().ok_or(PdfError::Invalid)?,
            "q" | "Q" => return Err(PdfError::Invalid),
            "Tf" => {
                let [Object::Name(name), size] = args else {
                    return Err(PdfError::Invalid);
                };
                if !number(size) {
                    return Err(PdfError::Invalid);
                }
                let defined = resource(doc, resources, b"Font", name)?
                    .as_dict()
                    .map_err(parser_error)?;
                font(doc, defined)?;
                if !encodings.contains_key(name.as_slice()) {
                    return Err(PdfError::Invalid);
                }
                current_font = Some(name);
                extractor_font = Some(name);
            }
            "Tj" | "TJ" | "'" | "\"" => {
                if !in_text || current_font.is_none() {
                    return Err(PdfError::Invalid);
                }
                if current_font != extractor_font {
                    return Err(PdfError::Unsupported);
                }
                let encoding = encodings
                    .get(current_font.ok_or(PdfError::Invalid)?)
                    .ok_or(PdfError::Invalid)?;
                match (op.operator.as_str(), args) {
                    ("Tj" | "'", [text]) => text_bytes(encoding, text)?,
                    ("\"", [word, character, text]) if number(word) && number(character) => {
                        text_bytes(encoding, text)?
                    }
                    ("TJ", [Object::Array(items)]) => {
                        for item in items {
                            if !number(item) {
                                text_bytes(encoding, item)?;
                            }
                        }
                    }
                    _ => return Err(PdfError::Invalid),
                }
            }
            "Do" => {
                let [Object::Name(name)] = args else {
                    return Err(PdfError::Invalid);
                };
                let object = resource(doc, resources, b"XObject", name)?
                    .as_stream()
                    .map_err(parser_error)?;
                match object
                    .dict
                    .get(b"Subtype")
                    .and_then(Object::as_name)
                    .map_err(parser_error)?
                {
                    b"Image" => {} // Explicitly no OCR; images do not hide PDF text operations.
                    _ => return Err(PdfError::Unsupported),
                }
            }
            "gs" => {
                let [Object::Name(name)] = args else {
                    return Err(PdfError::Invalid);
                };
                let state = resource(doc, resources, b"ExtGState", name)?
                    .as_dict()
                    .map_err(parser_error)?;
                if state.has(b"Font") {
                    return Err(PdfError::Unsupported);
                }
            }
            // Known non-text painting/layout/marked-content operations. Unknown
            // extensions may carry text and are not silently skipped.
            "cm" | "w" | "J" | "j" | "M" | "d" | "ri" | "i" | "m" | "l" | "c" | "v" | "y" | "h"
            | "re" | "S" | "s" | "f" | "F" | "f*" | "B" | "B*" | "b" | "b*" | "n" | "W" | "W*"
            | "CS" | "cs" | "SC" | "SCN" | "sc" | "scn" | "G" | "g" | "RG" | "rg" | "K" | "k"
            | "sh" | "BI" | "MP" | "DP" | "BMC" | "BDC" | "EMC" | "BX" | "EX" | "Tc" | "Tw"
            | "Tz" | "TL" | "Tr" | "Ts" | "Td" | "TD" | "Tm" | "T*" => {}
            _ => return Err(PdfError::Unsupported),
        }
    }
    if in_text || !stack.is_empty() {
        return Err(PdfError::Invalid);
    }
    Ok(())
}
