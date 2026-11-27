//! Safe Rust port of `src/codec/SkXmp.cpp` (self-contained safe XML DOM parser + XMP queries).

use crate::gainmap::{BaseImageType, GainmapInfo, GainmapType};

// ============================================================================
// Minimal, strict, safe XML 1.0 DOM parser matching SkXMLParser / SkDOM
// ============================================================================

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
enum XmlNodeKind {
    Element,
    Text,
}

#[derive(Clone, Debug)]
struct XmlNode {
    kind: XmlNodeKind,
    /// Element tag name if `Element`, or text content if `Text`.
    name: String,
    attrs: Vec<(String, String)>,
    children: Vec<XmlNode>,
}

impl XmlNode {
    fn find_attr(&self, attr_name: &str) -> Option<&str> {
        for (k, v) in &self.attrs {
            if k == attr_name {
                return Some(v.as_str());
            }
        }
        None
    }

    fn count_children(&self, name: Option<&str>) -> usize {
        match name {
            None => self.children.len(),
            Some(target) => self.children.iter().filter(|c| c.name == target).count(),
        }
    }

    fn first_child(&self, name: Option<&str>) -> Option<&XmlNode> {
        match name {
            None => self.children.first(),
            Some(target) => self.children.iter().find(|c| c.name == target),
        }
    }

    fn children_named<'a>(&'a self, name: &'a str) -> impl Iterator<Item = &'a XmlNode> {
        self.children.iter().filter(move |c| c.name == name)
    }
}

fn decode_xml_bytes(raw: &[u8]) -> Option<String> {
    if raw.starts_with(&[0xEF, 0xBB, 0xBF]) {
        let s = std::str::from_utf8(&raw[3..]).ok()?;
        return Some(normalize_line_endings(s));
    }
    if raw.starts_with(&[0xFE, 0xFF]) && raw.len() % 2 == 0 {
        let units: Vec<u16> = raw[2..]
            .chunks_exact(2)
            .map(|c| u16::from_be_bytes([c[0], c[1]]))
            .collect();
        let s = String::from_utf16(&units).ok()?;
        return Some(normalize_line_endings(&s));
    }
    if raw.starts_with(&[0xFF, 0xFE]) && raw.len() % 2 == 0 {
        let units: Vec<u16> = raw[2..]
            .chunks_exact(2)
            .map(|c| u16::from_le_bytes([c[0], c[1]]))
            .collect();
        let s = String::from_utf16(&units).ok()?;
        return Some(normalize_line_endings(&s));
    }
    let s = std::str::from_utf8(raw).ok()?;
    Some(normalize_line_endings(s))
}

fn normalize_line_endings(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\r' {
            if chars.peek() == Some(&'\n') {
                chars.next();
            }
            out.push('\n');
        } else {
            out.push(c);
        }
    }
    out
}

#[inline]
fn is_xml_ws(b: u8) -> bool {
    matches!(b, b' ' | b'\t' | b'\n' | b'\r')
}

#[inline]
fn is_name_start_char(c: char) -> bool {
    c == ':'
        || c == '_'
        || c.is_ascii_alphabetic()
        || ('\u{C0}'..='\u{D6}').contains(&c)
        || ('\u{D8}'..='\u{F6}').contains(&c)
        || ('\u{F8}'..='\u{2FF}').contains(&c)
        || ('\u{370}'..='\u{37D}').contains(&c)
        || ('\u{37F}'..='\u{1FFF}').contains(&c)
        || ('\u{200C}'..='\u{200D}').contains(&c)
        || ('\u{2070}'..='\u{218F}').contains(&c)
        || ('\u{2C00}'..='\u{2FEF}').contains(&c)
        || ('\u{3001}'..='\u{D7FF}').contains(&c)
        || ('\u{F900}'..='\u{FDCF}').contains(&c)
        || ('\u{FDF0}'..='\u{FFFD}').contains(&c)
        || ('\u{10000}'..='\u{EFFFF}').contains(&c)
}

#[inline]
fn is_name_char(c: char) -> bool {
    is_name_start_char(c)
        || c == '-'
        || c == '.'
        || c.is_ascii_digit()
        || c == '\u{B7}'
        || ('\u{0300}'..='\u{036F}').contains(&c)
        || ('\u{203F}'..='\u{2040}').contains(&c)
}

struct XmlParser<'a> {
    src: &'a str,
    pos: usize,
}

impl<'a> XmlParser<'a> {
    fn new(src: &'a str) -> Self {
        Self { src, pos: 0 }
    }

    fn rest(&self) -> &'a str {
        &self.src[self.pos..]
    }

    fn is_eof(&self) -> bool {
        self.pos >= self.src.len()
    }

    fn peek_byte(&self) -> Option<u8> {
        self.src.as_bytes().get(self.pos).copied()
    }

    fn starts_with(&self, pat: &str) -> bool {
        self.rest().starts_with(pat)
    }

    fn skip_ws(&mut self) {
        let bytes = self.src.as_bytes();
        while self.pos < bytes.len() && is_xml_ws(bytes[self.pos]) {
            self.pos += 1;
        }
    }

    fn parse_name(&mut self) -> Option<&'a str> {
        let start = self.pos;
        let mut iter = self.rest().char_indices();
        let (_, first) = iter.next()?;
        if !is_name_start_char(first) {
            return None;
        }
        let mut end = first.len_utf8();
        for (idx, c) in iter {
            if is_name_char(c) {
                end = idx + c.len_utf8();
            } else {
                break;
            }
        }
        self.pos = start + end;
        Some(&self.src[start..self.pos])
    }

    fn skip_comment(&mut self) -> bool {
        if !self.starts_with("<!--") {
            return false;
        }
        self.pos += 4;
        if let Some(rel) = self.rest().find("-->") {
            // XML 1.0 forbids "--" inside comment body.
            if self.rest()[..rel].contains("--") {
                return false;
            }
            self.pos += rel + 3;
            true
        } else {
            false
        }
    }

    fn skip_pi(&mut self) -> bool {
        if !self.starts_with("<?") {
            return false;
        }
        self.pos += 2;
        if self.parse_name().is_none() {
            return false;
        }
        if let Some(rel) = self.rest().find("?>") {
            self.pos += rel + 2;
            true
        } else {
            false
        }
    }

    fn skip_doctype(&mut self) -> bool {
        if !self.starts_with("<!DOCTYPE") {
            return false;
        }
        self.pos += 9;
        let mut depth = 0usize;
        let bytes = self.src.as_bytes();
        let mut quote: Option<u8> = None;
        while self.pos < bytes.len() {
            let b = bytes[self.pos];
            if let Some(q) = quote {
                self.pos += 1;
                if b == q {
                    quote = None;
                }
                continue;
            }
            if b == b'"' || b == b'\'' {
                quote = Some(b);
                self.pos += 1;
            } else if b == b'[' {
                depth += 1;
                self.pos += 1;
            } else if b == b']' {
                if depth == 0 {
                    return false;
                }
                depth -= 1;
                self.pos += 1;
            } else if depth > 0 && self.starts_with("<!ENTITY") {
                // Reject entity declarations (matches Expat CVE-2013-0340 protection).
                return false;
            } else if b == b'>' && depth == 0 {
                self.pos += 1;
                return true;
            } else {
                self.pos += 1;
            }
        }
        false
    }

    fn parse_entity_ref(&mut self) -> Option<char> {
        debug_assert!(self.starts_with("&"));
        self.pos += 1;
        let semi = self.rest().find(';')?;
        let body = &self.rest()[..semi];
        self.pos += semi + 1;
        match body {
            "lt" => Some('<'),
            "gt" => Some('>'),
            "amp" => Some('&'),
            "quot" => Some('"'),
            "apos" => Some('\''),
            _ if body.starts_with("#x") || body.starts_with("#X") => {
                let code = u32::from_str_radix(&body[2..], 16).ok()?;
                if code == 0 {
                    return None;
                }
                char::from_u32(code)
            }
            _ if body.starts_with('#') => {
                let code = body[1..].parse::<u32>().ok()?;
                if code == 0 {
                    return None;
                }
                char::from_u32(code)
            }
            _ => None,
        }
    }

    fn parse_attr_value(&mut self) -> Option<String> {
        let quote = match self.peek_byte()? {
            b'"' => b'"',
            b'\'' => b'\'',
            _ => return None,
        };
        self.pos += 1;
        let mut out = String::new();
        while !self.is_eof() {
            let b = self.peek_byte()?;
            if b == quote {
                self.pos += 1;
                return Some(out);
            }
            if b == b'<' || b == 0 {
                return None;
            }
            if b == b'&' {
                let c = self.parse_entity_ref()?;
                out.push(c);
            } else {
                let c = self.rest().chars().next()?;
                self.pos += c.len_utf8();
                if c == '\t' || c == '\n' || c == '\r' {
                    out.push(' ');
                } else {
                    out.push(c);
                }
            }
        }
        None
    }

    fn parse_document(&mut self) -> Option<XmlNode> {
        // Reject NUL bytes anywhere (Expat rejects them).
        if self.src.as_bytes().contains(&0) {
            return None;
        }

        // Prolog: whitespace, comments, PIs, optional DOCTYPE.
        loop {
            self.skip_ws();
            if self.starts_with("<!--") {
                if !self.skip_comment() {
                    return None;
                }
            } else if self.starts_with("<?") {
                if !self.skip_pi() {
                    return None;
                }
            } else if self.starts_with("<!DOCTYPE") {
                if !self.skip_doctype() {
                    return None;
                }
            } else {
                break;
            }
        }

        let root = self.parse_element()?;

        // Misc after root element: whitespace, comments, PIs only.
        loop {
            self.skip_ws();
            if self.is_eof() {
                break;
            }
            if self.starts_with("<!--") {
                if !self.skip_comment() {
                    return None;
                }
            } else if self.starts_with("<?") {
                if !self.skip_pi() {
                    return None;
                }
            } else {
                return None;
            }
        }

        Some(root)
    }

    fn parse_element(&mut self) -> Option<XmlNode> {
        if !self.starts_with("<") {
            return None;
        }
        self.pos += 1;
        let tag_name = self.parse_name()?.to_string();
        let mut attrs: Vec<(String, String)> = Vec::new();

        let self_closing;
        loop {
            let before_ws = self.pos;
            self.skip_ws();
            let had_ws = self.pos > before_ws;
            if self.starts_with("/>") {
                self.pos += 2;
                self_closing = true;
                break;
            }
            if self.starts_with(">") {
                self.pos += 1;
                self_closing = false;
                break;
            }
            if !had_ws {
                return None;
            }
            let attr_name = self.parse_name()?.to_string();
            if attrs.iter().any(|(k, _)| k == &attr_name) {
                return None;
            }
            self.skip_ws();
            if !self.starts_with("=") {
                return None;
            }
            self.pos += 1;
            self.skip_ws();
            let attr_val = self.parse_attr_value()?;
            attrs.push((attr_name, attr_val));
        }

        let mut children = Vec::new();
        if !self_closing {
            let mut buffered_text = String::new();
            let flush_text = |buf: &mut String, out: &mut Vec<XmlNode>| {
                if !buf.is_empty() {
                    out.push(XmlNode {
                        kind: XmlNodeKind::Text,
                        name: std::mem::take(buf),
                        attrs: Vec::new(),
                        children: Vec::new(),
                    });
                }
            };

            loop {
                if self.is_eof() {
                    return None;
                }
                if self.starts_with("</") {
                    flush_text(&mut buffered_text, &mut children);
                    self.pos += 2;
                    let close_name = self.parse_name()?;
                    if close_name != tag_name {
                        return None;
                    }
                    self.skip_ws();
                    if !self.starts_with(">") {
                        return None;
                    }
                    self.pos += 1;
                    break;
                } else if self.starts_with("<!--") {
                    if !self.skip_comment() {
                        return None;
                    }
                } else if self.starts_with("<?") {
                    if !self.skip_pi() {
                        return None;
                    }
                } else if self.starts_with("<![CDATA[") {
                    self.pos += 9;
                    let end = self.rest().find("]]>")?;
                    buffered_text.push_str(&self.rest()[..end]);
                    self.pos += end + 3;
                } else if self.starts_with("<") {
                    flush_text(&mut buffered_text, &mut children);
                    let child = self.parse_element()?;
                    children.push(child);
                } else if self.starts_with("&") {
                    let c = self.parse_entity_ref()?;
                    buffered_text.push(c);
                } else {
                    // Consume chunk up to next '<' or '&'.
                    let start = self.pos;
                    let bytes = self.src.as_bytes();
                    while self.pos < bytes.len()
                        && bytes[self.pos] != b'<'
                        && bytes[self.pos] != b'&'
                    {
                        self.pos += 1;
                    }
                    let chunk = &self.src[start..self.pos];
                    if chunk.contains("]]>") {
                        return None;
                    }
                    buffered_text.push_str(chunk);
                }
            }
        }

        Some(XmlNode {
            kind: XmlNodeKind::Element,
            name: tag_name,
            attrs,
            children,
        })
    }
}

fn parse_xml(raw: &[u8]) -> Option<XmlNode> {
    let text = decode_xml_bytes(raw)?;
    let mut parser = XmlParser::new(&text);
    parser.parse_document()
}

// ============================================================================
// SkParse-compatible value parsing helpers
// ============================================================================

#[inline]
fn is_skparse_ws(b: u8) -> bool {
    (1..=32).contains(&b)
}

fn skip_skparse_ws(s: &str) -> &str {
    let bytes = s.as_bytes();
    let mut i = 0;
    while i < bytes.len() && is_skparse_ws(bytes[i]) {
        i += 1;
    }
    &s[i..]
}

fn parse_sk_s32(s: &str) -> Option<i32> {
    let s = skip_skparse_ws(s);
    let bytes = s.as_bytes();
    let mut idx = 0;
    let mut sign: i64 = 1;
    let mut max_abs: i64 = i32::MAX as i64;
    if bytes.get(idx) == Some(&b'-') {
        sign = -1;
        max_abs = -(i32::MIN as i64);
        idx += 1;
    }
    if !bytes.get(idx).map_or(false, |b| b.is_ascii_digit()) {
        return None;
    }
    let mut n: i64 = 0;
    while let Some(&b) = bytes.get(idx) {
        if !b.is_ascii_digit() {
            break;
        }
        n = 10 * n + (b - b'0') as i64;
        if n > max_abs {
            return None;
        }
        idx += 1;
    }
    Some((sign * n) as i32)
}

/// Matches `SkParse::FindScalar` (which calls `strtod` after skipping bytes `1..=32`).
fn parse_sk_scalar(s: &str) -> Option<f32> {
    let s = skip_skparse_ws(s);
    let bytes = s.as_bytes();
    let mut i = 0;
    if matches!(bytes.get(i), Some(b'+' | b'-')) {
        i += 1;
    }
    let mut has_digits = false;
    while matches!(bytes.get(i), Some(b'0'..=b'9')) {
        has_digits = true;
        i += 1;
    }
    if bytes.get(i) == Some(&b'.') {
        i += 1;
        while matches!(bytes.get(i), Some(b'0'..=b'9')) {
            has_digits = true;
            i += 1;
        }
    }
    if !has_digits {
        return None;
    }
    let exp_start = i;
    if matches!(bytes.get(i), Some(b'e' | b'E')) {
        i += 1;
        if matches!(bytes.get(i), Some(b'+' | b'-')) {
            i += 1;
        }
        let mut exp_digits = false;
        while matches!(bytes.get(i), Some(b'0'..=b'9')) {
            exp_digits = true;
            i += 1;
        }
        if !exp_digits {
            i = exp_start;
        }
    }
    let token = &s[..i];
    let v: f64 = token.parse().ok()?;
    Some(v as f32)
}

// ============================================================================
// XMP traversal helpers matching SkXmp.cpp
// ============================================================================

const XMLNS_PREFIX: &str = "xmlns:";

fn get_namespace_prefix(name: &str) -> Option<&str> {
    if name.len() <= XMLNS_PREFIX.len() {
        return None;
    }
    name.strip_prefix(XMLNS_PREFIX)
}

fn get_unique_child_text<'a>(node: &'a XmlNode, child_name: &str) -> Option<&'a str> {
    if node.count_children(Some(child_name)) != 1 {
        return None;
    }
    let child = node.first_child(Some(child_name))?;
    if child.count_children(None) != 1 {
        return None;
    }
    let grand_child = child.first_child(None)?;
    if grand_child.kind != XmlNodeKind::Text {
        return None;
    }
    Some(grand_child.name.as_str())
}

fn get_typed_child<'a>(node: &'a XmlNode, prefix: &str, ty: &str) -> Option<&'a XmlNode> {
    let name = format!("{}:{}", prefix, ty);
    if let Some(child) = node.first_child(Some(&name)) {
        return Some(child);
    }
    let type_child = node.first_child(Some("rdf:type"))?;
    let resource = type_child.find_attr("rdf:resource")?;
    if resource != ty {
        return None;
    }
    let value_child = node.first_child(Some("rdf:value"))?;
    let parse_type = value_child.find_attr("rdf:parseType")?;
    if parse_type != "Resource" {
        return None;
    }
    Some(value_child)
}

fn get_attr<'a>(node: &'a XmlNode, prefix: &str, key: &str) -> Option<&'a str> {
    let name = format!("{}:{}", prefix, key);
    if let Some(val) = node.find_attr(&name) {
        return Some(val);
    }
    get_unique_child_text(node, &name)
}

fn get_attr_bool(node: &XmlNode, prefix: &str, key: &str) -> Option<bool> {
    let attr = get_attr(node, prefix, key)?;
    match attr {
        "False" => Some(false),
        "True" => Some(true),
        _ => None,
    }
}

fn get_attr_int32(node: &XmlNode, prefix: &str, key: &str) -> Option<i32> {
    let attr = get_attr(node, prefix, key)?;
    parse_sk_s32(attr)
}

fn get_attr_float(node: &XmlNode, prefix: &str, key: &str) -> Option<f32> {
    let attr = get_attr(node, prefix, key)?;
    parse_sk_scalar(attr)
}

fn get_attr_float3_as_list(node: &XmlNode, prefix: &str, key: &str) -> Option<[f32; 4]> {
    let name = format!("{}:{}", prefix, key);
    if node.count_children(Some(&name)) != 1 {
        return None;
    }
    let child = node.first_child(Some(&name))?;
    let seq = child.first_child(Some("rdf:Seq"))?;

    let mut count = 0usize;
    let mut values = [0.0_f32; 3];
    for li_node in seq.children_named("rdf:li") {
        if count > 2 {
            return None;
        }
        if li_node.count_children(None) != 1 {
            return None;
        }
        let li_text_node = li_node.first_child(None)?;
        if li_text_node.kind != XmlNodeKind::Text {
            return None;
        }
        values[count] = parse_sk_scalar(&li_text_node.name)?;
        count += 1;
    }
    if count < 3 {
        return None;
    }
    Some([values[0], values[1], values[2], 1.0_f32])
}

fn get_attr_float3(node: &XmlNode, prefix: &str, key: &str) -> Option<[f32; 4]> {
    if let Some(v) = get_attr_float3_as_list(node, prefix, key) {
        return Some(v);
    }
    let v = get_attr_float(node, prefix, key)?;
    Some([v, v, v, 1.0_f32])
}

fn collect_uri_namespaces<'a>(node: &'a XmlNode, uris: &[&str], out: &mut [Option<&'a str>]) {
    for (attr_name, attr_val) in &node.attrs {
        if attr_name.len() <= XMLNS_PREFIX.len() || !attr_name.starts_with(XMLNS_PREFIX) {
            continue;
        }
        for (i, &uri) in uris.iter().enumerate() {
            if attr_val == uri {
                out[i] = Some(attr_name.as_str());
            }
        }
    }
}

fn find_uri_namespaces_in_dom<'a>(
    root: &'a XmlNode,
    uris: &[&str],
) -> Option<(&'a XmlNode, Vec<&'a str>)> {
    if root.name != "x:xmpmeta" {
        return None;
    }
    for rdf in root.children_named("rdf:RDF") {
        let mut rdf_ns: Vec<Option<&'a str>> = vec![None; uris.len()];
        collect_uri_namespaces(rdf, uris, &mut rdf_ns);

        for desc in rdf.children_named("rdf:Description") {
            let mut desc_ns = rdf_ns.clone();
            collect_uri_namespaces(desc, uris, &mut desc_ns);
            if desc_ns.iter().all(|ns| ns.is_some()) {
                let resolved: Vec<&'a str> = desc_ns.into_iter().map(|o| o.unwrap()).collect();
                return Some((desc, resolved));
            }
        }
    }
    None
}

/// Safe Rust representation of `SkXmpImpl`.
pub struct XmpDocument {
    standard: XmlNode,
    extended: Option<XmlNode>,
    /// Cached NUL-terminated UTF-8 bytes of `HasExtendedXMP` GUID (if present in `standard`).
    extended_guid_cstr: Option<Vec<u8>>,
}

impl XmpDocument {
    pub fn from_bytes(standard: &[u8], extended: Option<&[u8]>) -> Option<Self> {
        let std_dom = parse_xml(standard)?;
        let ext_dom = extended.and_then(parse_xml);
        let extended_guid_cstr = Self::extract_extended_guid(&std_dom).map(|s| {
            let mut v = s.as_bytes().to_vec();
            v.push(0);
            v
        });
        Some(Self {
            standard: std_dom,
            extended: ext_dom,
            extended_guid_cstr,
        })
    }

    fn extract_extended_guid(standard: &XmlNode) -> Option<&str> {
        let uris = ["http://ns.adobe.com/xmp/note/"];
        let (node, ns) = find_uri_namespaces_in_dom(standard, &uris)?;
        let prefix = get_namespace_prefix(ns[0])?;
        get_attr(node, prefix, "HasExtendedXMP")
    }

    fn find_uri_namespaces<'a>(&'a self, uris: &[&str]) -> Option<(&'a XmlNode, Vec<&'a str>)> {
        if let Some(found) = find_uri_namespaces_in_dom(&self.standard, uris) {
            return Some(found);
        }
        if let Some(ref ext) = self.extended {
            if let Some(found) = find_uri_namespaces_in_dom(ext, uris) {
                return Some(found);
            }
        }
        None
    }

    /// Matches `SkXmp::getExtendedXmpGuid` (returns NUL-terminated bytes).
    pub fn extended_xmp_guid_cstr(&self) -> Option<&[u8]> {
        self.extended_guid_cstr.as_deref()
    }

    /// Matches `SkXmp::getExtendedXmpGuid` as a Rust `&str`.
    pub fn extended_xmp_guid(&self) -> Option<&str> {
        let bytes = self.extended_guid_cstr.as_deref()?;
        std::str::from_utf8(&bytes[..bytes.len().saturating_sub(1)]).ok()
    }

    /// Matches `SkXmp::getContainerGainmapLocation`.
    pub fn get_container_gainmap_location(&self) -> Option<(usize, usize)> {
        let uris = [
            "http://ns.google.com/photos/1.0/container/",
            "http://ns.google.com/photos/1.0/container/item/",
        ];
        let (node, ns) = self.find_uri_namespaces(&uris)?;
        let container_prefix = get_namespace_prefix(ns[0])?;
        let item_prefix = get_namespace_prefix(ns[1])?;

        let directory = get_typed_child(node, container_prefix, "Directory")?;
        let seq = directory.first_child(Some("rdf:Seq"))?;

        let mut is_first_item = true;
        let mut offset = 0usize;
        for li in seq.children_named("rdf:li") {
            let item = get_typed_child(li, container_prefix, "Item")?;
            let item_semantic = get_attr(item, item_prefix, "Semantic")?;
            let item_mime = get_attr(item, item_prefix, "Mime")?;

            if is_first_item {
                is_first_item = false;
                if item_semantic != "Primary" || item_mime != "image/jpeg" {
                    return None;
                }
                if let Some(padding) = get_attr_int32(item, item_prefix, "Padding") {
                    if padding < 0 {
                        return None;
                    }
                    offset = offset.checked_add(padding as usize)?;
                }
            } else {
                let length = get_attr_int32(item, item_prefix, "Length")?;
                if length < 0 {
                    return None;
                }
                if item_semantic != "GainMap" {
                    offset = offset.checked_add(length as usize)?;
                    continue;
                }
                if item_mime != "image/jpeg" {
                    return None;
                }
                return Some((offset, length as usize));
            }
        }
        None
    }

    /// Matches `SkXmp::getGainmapInfoApple`.
    pub fn get_gainmap_info_apple(&self, exif_hdr_headroom: f32) -> Option<GainmapInfo> {
        let uris = [
            "http://ns.apple.com/pixeldatainfo/1.0/",
            "http://ns.apple.com/HDRGainMap/1.0/",
        ];
        let (node, ns) = self.find_uri_namespaces(&uris)?;
        let adpi_prefix = get_namespace_prefix(ns[0])?;
        let hdr_gainmap_prefix = get_namespace_prefix(ns[1])?;

        let aux_type = get_attr(node, adpi_prefix, "AuxiliaryImageType")?;
        if aux_type != "urn:com:apple:photo:2020:aux:hdrgainmap" {
            return None;
        }
        let _version = get_attr_int32(node, hdr_gainmap_prefix, "HDRGainMapVersion")?;

        let hdr_headroom = get_attr_float(node, hdr_gainmap_prefix, "HDRGainMapHeadroom")
            .unwrap_or(exif_hdr_headroom);

        Some(GainmapInfo {
            gainmap_ratio_min: [1.0, 1.0, 1.0, 1.0],
            gainmap_ratio_max: [hdr_headroom, hdr_headroom, hdr_headroom, 1.0],
            gainmap_gamma: [1.0, 1.0, 1.0, 1.0],
            epsilon_sdr: [0.0, 0.0, 0.0, 1.0],
            epsilon_hdr: [0.0, 0.0, 0.0, 1.0],
            display_ratio_sdr: 1.0,
            display_ratio_hdr: hdr_headroom,
            base_image_type: BaseImageType::Sdr,
            gainmap_type: GainmapType::Apple,
            has_gainmap_math_color_space: false,
        })
    }

    /// Matches `SkXmp::getGainmapInfoAdobe`.
    /// Note: preserves `gainmap_type` and `has_gainmap_math_color_space` as default (caller on C++
    /// side only overwrites the fields `SkXmpImpl::getGainmapInfoAdobe` writes).
    pub fn get_gainmap_info_adobe(&self) -> Option<GainmapInfo> {
        let uris = ["http://ns.adobe.com/hdr-gain-map/1.0/"];
        let (node, ns) = self.find_uri_namespaces(&uris)?;
        let hdrgm_prefix = get_namespace_prefix(ns[0])?;

        let version = get_attr(node, hdrgm_prefix, "Version")?;
        if version != "1.0" {
            return None;
        }

        let base_rendition_is_hdr =
            get_attr_bool(node, hdrgm_prefix, "BaseRenditionIsHDR").unwrap_or(false);
        let gain_map_min =
            get_attr_float3(node, hdrgm_prefix, "GainMapMin").unwrap_or([0.0, 0.0, 0.0, 1.0]);
        let gain_map_max =
            get_attr_float3(node, hdrgm_prefix, "GainMapMax").unwrap_or([1.0, 1.0, 1.0, 1.0]);
        let gamma = get_attr_float3(node, hdrgm_prefix, "Gamma").unwrap_or([1.0, 1.0, 1.0, 1.0]);
        let offset_sdr = get_attr_float3(node, hdrgm_prefix, "OffsetSDR")
            .unwrap_or([1.0 / 64.0, 1.0 / 64.0, 1.0 / 64.0, 0.0]);
        let offset_hdr = get_attr_float3(node, hdrgm_prefix, "OffsetHDR")
            .unwrap_or([1.0 / 64.0, 1.0 / 64.0, 1.0 / 64.0, 0.0]);
        let hdr_capacity_min = get_attr_float(node, hdrgm_prefix, "HDRCapacityMin").unwrap_or(0.0);
        let hdr_capacity_max = get_attr_float(node, hdrgm_prefix, "HDRCapacityMax").unwrap_or(1.0);

        let k_log2 = 2.0_f32.ln();
        Some(GainmapInfo {
            gainmap_ratio_min: [
                (gain_map_min[0] * k_log2).exp(),
                (gain_map_min[1] * k_log2).exp(),
                (gain_map_min[2] * k_log2).exp(),
                1.0,
            ],
            gainmap_ratio_max: [
                (gain_map_max[0] * k_log2).exp(),
                (gain_map_max[1] * k_log2).exp(),
                (gain_map_max[2] * k_log2).exp(),
                1.0,
            ],
            gainmap_gamma: [1.0 / gamma[0], 1.0 / gamma[1], 1.0 / gamma[2], 1.0],
            epsilon_sdr: offset_sdr,
            epsilon_hdr: offset_hdr,
            display_ratio_sdr: (hdr_capacity_min * k_log2).exp(),
            display_ratio_hdr: (hdr_capacity_max * k_log2).exp(),
            base_image_type: if base_rendition_is_hdr {
                BaseImageType::Hdr
            } else {
                BaseImageType::Sdr
            },
            gainmap_type: GainmapType::Default,
            has_gainmap_math_color_space: false,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn invalid_xml_returns_none() {
        let bad = br#"
            <x:xmpmeta xmlns:x="adobe:ns:meta/" x:xmptk="XMP Core 6.0.0">
               <rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#"
                        xmlns:"#;
        assert!(XmpDocument::from_bytes(bad, None).is_none());
    }

    #[test]
    fn adobe_hdrgm_fields_and_lists() {
        let xml = br#"
            <?xpacket begin="" id="W5M0MpCehiHzreSzNTczkc9d"?>
            <x:xmpmeta xmlns:x="adobe:ns:meta/" x:xmptk="XMP Core 6.0.0">
               <rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#"
                        xmlns:hdrgm="http://ns.adobe.com/hdr-gain-map/1.0/">
                  <rdf:Description rdf:about=""
                     hdrgm:Version="1.0"
                     hdrgm:GainMapMin="2.0"
                     hdrgm:OffsetSDR="0.1">
                     <hdrgm:GainMapMax>
                       <rdf:Seq>
                         <rdf:li>3</rdf:li>
                         <rdf:li>4</rdf:li>
                         <rdf:li>5</rdf:li>
                       </rdf:Seq>
                     </hdrgm:GainMapMax>
                     <hdrgm:Gamma>
                       1.2
                     </hdrgm:Gamma>
                     <hdrgm:OffsetHDR>
                       <rdf:Seq>
                         <rdf:li> 0.2 </rdf:li>
                         <rdf:li> 0.3 </rdf:li>
                         <rdf:li> 0.4 </rdf:li>
                       </rdf:Seq>
                     </hdrgm:OffsetHDR>
                  </rdf:Description>
               </rdf:RDF>
            </x:xmpmeta>
            <?xpacket end="w"?>"#;
        let doc = XmpDocument::from_bytes(xml, None).unwrap();
        let info = doc.get_gainmap_info_adobe().unwrap();
        assert!((info.gainmap_ratio_min[0] - 4.0).abs() < 1e-5);
        assert!((info.gainmap_ratio_max[0] - 8.0).abs() < 1e-5);
        assert!((info.gainmap_ratio_max[1] - 16.0).abs() < 1e-5);
        assert!((info.gainmap_ratio_max[2] - 32.0).abs() < 1e-5);
        assert!((info.gainmap_gamma[0] - 1.0 / 1.2).abs() < 1e-6);
        assert_eq!(info.epsilon_sdr[0], 0.1);
        assert_eq!(info.epsilon_hdr[0], 0.2);
        assert_eq!(info.epsilon_hdr[1], 0.3);
        assert_eq!(info.epsilon_hdr[2], 0.4);
    }

    #[test]
    fn container_typed_node_and_rdf_equivalent() {
        let xml = br#"
            <x:xmpmeta xmlns:x="adobe:ns:meta/">
             <rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#">
              <rdf:Description rdf:about=""
                xmlns:Container="http://ns.google.com/photos/1.0/container/"
                xmlns:Item="http://ns.google.com/photos/1.0/container/item/">
               <Container:Directory>
                <rdf:Seq>
                 <rdf:li rdf:parseType="Resource">
                  <rdf:value rdf:parseType="Resource">
                   <Item:Mime>image/jpeg</Item:Mime>
                   <Item:Semantic>Primary</Item:Semantic>
                   <Item:Padding>16</Item:Padding>
                  </rdf:value>
                  <rdf:type rdf:resource="Item"/>
                 </rdf:li>
                 <rdf:li rdf:parseType="Resource">
                  <rdf:value rdf:parseType="Resource">
                   <Item:Semantic>GainMap</Item:Semantic>
                   <Item:Mime>image/jpeg</Item:Mime>
                   <Item:Length>49035</Item:Length>
                  </rdf:value>
                  <rdf:type rdf:resource="Item"/>
                 </rdf:li>
                </rdf:Seq>
               </Container:Directory>
              </rdf:Description>
             </rdf:RDF>
            </x:xmpmeta>"#;
        let doc = XmpDocument::from_bytes(xml, None).unwrap();
        assert_eq!(doc.get_container_gainmap_location(), Some((16, 49035)));
    }

    #[test]
    fn extended_xmp_guid_and_apple_gainmap() {
        let std_xml = br#"
            <x:xmpmeta xmlns:x="adobe:ns:meta/">
              <rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#">
                <rdf:Description xmlns:xmpNote="http://ns.adobe.com/xmp/note/"
                  xmpNote:HasExtendedXMP="0123456789ABCDEF0123456789ABCDEF"/>
              </rdf:RDF>
            </x:xmpmeta>"#;
        let ext_xml = br#"
            <x:xmpmeta xmlns:x="adobe:ns:meta/">
              <rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#">
                <rdf:Description
                  xmlns:apdi="http://ns.apple.com/pixeldatainfo/1.0/"
                  xmlns:HDRGainMap="http://ns.apple.com/HDRGainMap/1.0/"
                  apdi:AuxiliaryImageType="urn:com:apple:photo:2020:aux:hdrgainmap"
                  HDRGainMap:HDRGainMapVersion="65536"/>
              </rdf:RDF>
            </x:xmpmeta>"#;
        let doc = XmpDocument::from_bytes(std_xml, Some(ext_xml)).unwrap();
        assert_eq!(
            doc.extended_xmp_guid(),
            Some("0123456789ABCDEF0123456789ABCDEF")
        );
        let info = doc.get_gainmap_info_apple(3.5).unwrap();
        assert_eq!(info.gainmap_type, GainmapType::Apple);
        assert_eq!(info.display_ratio_hdr, 3.5);
        assert_eq!(info.gainmap_ratio_max[0], 3.5);
    }

    #[test]
    fn entity_decl_rejected_and_cdata_entities_decoded() {
        let dtd = br#"<!DOCTYPE foo [<!ENTITY xxe "bad">]><x:xmpmeta/>"#;
        assert!(XmpDocument::from_bytes(dtd, None).is_none());

        let xml = br#"
            <x:xmpmeta xmlns:x="adobe:ns:meta/">
              <rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#">
                <rdf:Description xmlns:hdrgm="http://ns.adobe.com/hdr-gain-map/1.0/">
                  <hdrgm:Version><![CDATA[1.0]]></hdrgm:Version>
                  <hdrgm:GainMapMax>3<!-- inline comment -->.0</hdrgm:GainMapMax>
                </rdf:Description>
              </rdf:RDF>
            </x:xmpmeta>"#;
        let doc = XmpDocument::from_bytes(xml, None).unwrap();
        let info = doc.get_gainmap_info_adobe().unwrap();
        assert!((info.gainmap_ratio_max[0] - 8.0).abs() < 1e-5);
    }
}
