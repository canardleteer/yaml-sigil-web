//! Line-oriented YAML highlighting that keeps multi-document streams intact.

#[derive(Clone, Copy)]
enum Kind {
    Comment,
    Key,
    String,
    Number,
    Bool,
    Null,
    Punct,
    Doc,
    Anchor,
    Tag,
}

impl Kind {
    fn class(self) -> &'static str {
        match self {
            Self::Comment => "yaml-comment",
            Self::Key => "yaml-key",
            Self::String => "yaml-string",
            Self::Number => "yaml-number",
            Self::Bool => "yaml-bool",
            Self::Null => "yaml-null",
            Self::Punct => "yaml-punct",
            Self::Doc => "yaml-doc",
            Self::Anchor => "yaml-anchor",
            Self::Tag => "yaml-tag",
        }
    }
}

pub fn html(source: &str) -> String {
    let mut out = String::with_capacity(source.len().saturating_mul(2));
    let mut rest = source;
    while !rest.is_empty() {
        let (line, next) = split_line(rest);
        highlight_line(line, &mut out);
        rest = next;
    }
    out
}

fn split_line(source: &str) -> (&str, &str) {
    match source.find('\n') {
        Some(index) => source.split_at(index + 1),
        None => (source, ""),
    }
}

fn highlight_line(line: &str, out: &mut String) {
    let (body, newline) = trim_newline(line);
    if let Some(kind) = doc_marker_prefix(body) {
        push_span(out, Kind::Doc, &body[..kind]);
        tokenize_flow(&body[kind..], out);
    } else if is_directive(body) {
        push_span(out, Kind::Doc, body);
    } else {
        tokenize_flow(body, out);
    }
    out.push_str(newline);
}

fn trim_newline(line: &str) -> (&str, &str) {
    if let Some(body) = line.strip_suffix("\r\n") {
        (body, "\n")
    } else if let Some(body) = line.strip_suffix('\n') {
        (body, "\n")
    } else if let Some(body) = line.strip_suffix('\r') {
        (body, "\n")
    } else {
        (line, "")
    }
}

fn doc_marker_prefix(line: &str) -> Option<usize> {
    for marker in ["---", "..."] {
        if let Some(rest) = line.strip_prefix(marker)
            && (rest.is_empty() || rest.starts_with(|c: char| c.is_ascii_whitespace()))
        {
            return Some(marker.len());
        }
    }
    None
}

fn is_directive(line: &str) -> bool {
    line.starts_with("%YAML") || line.starts_with("%TAG")
}

fn tokenize_flow(line: &str, out: &mut String) {
    let bytes = line.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i].is_ascii_whitespace() {
            let start = i;
            while i < bytes.len() && bytes[i].is_ascii_whitespace() {
                i += 1;
            }
            escape_into(out, &line[start..i]);
            continue;
        }
        if bytes[i] == b'#' {
            push_span(out, Kind::Comment, &line[i..]);
            return;
        }
        if bytes[i] == b'"' || bytes[i] == b'\'' {
            let end = scan_quoted(line, i);
            push_span(out, Kind::String, &line[i..end]);
            i = end;
            continue;
        }
        if bytes[i] == b'&' || bytes[i] == b'*' {
            let end = scan_token(line, i + 1);
            push_span(out, Kind::Anchor, &line[i..end]);
            i = end;
            continue;
        }
        if bytes[i] == b'!' {
            let end = scan_tag(line, i);
            push_span(out, Kind::Tag, &line[i..end]);
            i = end;
            continue;
        }
        if matches!(bytes[i], b'|' | b'>')
            && (i + 1 == bytes.len()
                || bytes[i + 1].is_ascii_whitespace()
                || matches!(bytes[i + 1], b'-' | b'+' | b'1'..=b'9'))
        {
            let mut end = i + 1;
            if end < bytes.len() && matches!(bytes[end], b'-' | b'+' | b'1'..=b'9') {
                end += 1;
            }
            push_span(out, Kind::Punct, &line[i..end]);
            i = end;
            continue;
        }
        if bytes[i] == b'-' && (i + 1 == bytes.len() || bytes[i + 1].is_ascii_whitespace()) {
            push_span(out, Kind::Punct, "-");
            i += 1;
            continue;
        }
        if matches!(bytes[i], b'{' | b'}' | b'[' | b']' | b',' | b':') {
            push_span(out, Kind::Punct, &line[i..i + 1]);
            i += 1;
            continue;
        }
        if let Some(end) = scan_key(line, i) {
            push_span(out, Kind::Key, &line[i..end]);
            i = end;
            continue;
        }
        if let Some((kind, end)) = scan_scalar(line, i) {
            push_span(out, kind, &line[i..end]);
            i = end;
            continue;
        }
        let end = scan_plain(line, i);
        push_span(out, Kind::String, &line[i..end]);
        i = end;
    }
}

fn scan_quoted(line: &str, start: usize) -> usize {
    let bytes = line.as_bytes();
    let quote = bytes[start];
    let mut i = start + 1;
    while i < bytes.len() {
        if bytes[i] == b'\\' && quote == b'"' && i + 1 < bytes.len() {
            i += 2;
            continue;
        }
        if bytes[i] == quote {
            return i + 1;
        }
        i += 1;
    }
    bytes.len()
}

fn scan_token(line: &str, start: usize) -> usize {
    let bytes = line.as_bytes();
    let mut i = start;
    while i < bytes.len()
        && !bytes[i].is_ascii_whitespace()
        && !matches!(bytes[i], b',' | b':' | b'#' | b'[' | b']' | b'{' | b'}')
    {
        i += 1;
    }
    i
}

fn scan_tag(line: &str, start: usize) -> usize {
    let bytes = line.as_bytes();
    let mut i = start + 1;
    if i < bytes.len() && bytes[i] == b'<' {
        i += 1;
        while i < bytes.len() && bytes[i] != b'>' {
            i += 1;
        }
        if i < bytes.len() {
            i += 1;
        }
        return i;
    }
    scan_token(line, start)
}

fn scan_key(line: &str, start: usize) -> Option<usize> {
    let bytes = line.as_bytes();
    if !is_key_start(bytes[start]) {
        return None;
    }
    let mut i = start + 1;
    while i < bytes.len() && is_key_char(bytes[i]) {
        i += 1;
    }
    if i >= bytes.len() || bytes[i] != b':' {
        return None;
    }
    let after = i + 1;
    if after == bytes.len() || bytes[after].is_ascii_whitespace() || bytes[after] == b'#' {
        Some(i)
    } else {
        None
    }
}

fn is_key_start(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_' || b == b'.'
}

fn is_key_char(b: u8) -> bool {
    is_key_start(b) || b == b'-'
}

fn scan_scalar(line: &str, start: usize) -> Option<(Kind, usize)> {
    let rest = &line[start..];
    if let Some(end) = match_word(rest, &["true", "false", "True", "False", "TRUE", "FALSE"]) {
        return Some((Kind::Bool, start + end));
    }
    if let Some(end) = match_word(rest, &["null", "Null", "NULL", "~"]) {
        return Some((Kind::Null, start + end));
    }
    if let Some(end) = scan_number(rest) {
        return Some((Kind::Number, start + end));
    }
    None
}

fn match_word(rest: &str, words: &[&str]) -> Option<usize> {
    for word in words {
        if let Some(after) = rest.strip_prefix(word)
            && (after.is_empty()
                || after.starts_with(|c: char| !c.is_ascii_alphanumeric() && c != '_'))
        {
            return Some(word.len());
        }
    }
    None
}

fn scan_number(rest: &str) -> Option<usize> {
    let bytes = rest.as_bytes();
    let mut i = 0;
    if i < bytes.len() && (bytes[i] == b'+' || bytes[i] == b'-') {
        i += 1;
    }
    let digits_start = i;
    while i < bytes.len() && bytes[i].is_ascii_digit() {
        i += 1;
    }
    if i == digits_start {
        return None;
    }
    if i < bytes.len() && bytes[i] == b'.' {
        i += 1;
        let frac = i;
        while i < bytes.len() && bytes[i].is_ascii_digit() {
            i += 1;
        }
        if i == frac {
            return None;
        }
    }
    if i < bytes.len() && (bytes[i] == b'e' || bytes[i] == b'E') {
        let mut exp = i + 1;
        if exp < bytes.len() && (bytes[exp] == b'+' || bytes[exp] == b'-') {
            exp += 1;
        }
        let exp_digits = exp;
        while exp < bytes.len() && bytes[exp].is_ascii_digit() {
            exp += 1;
        }
        if exp == exp_digits {
            return None;
        }
        i = exp;
    }
    if i < bytes.len() && (bytes[i].is_ascii_alphanumeric() || bytes[i] == b'_') {
        return None;
    }
    Some(i)
}

fn scan_plain(line: &str, start: usize) -> usize {
    let bytes = line.as_bytes();
    let mut i = start + 1;
    while i < bytes.len()
        && !bytes[i].is_ascii_whitespace()
        && !matches!(bytes[i], b'#' | b',' | b':' | b'[' | b']' | b'{' | b'}')
    {
        i += 1;
    }
    i
}

fn push_span(out: &mut String, kind: Kind, text: &str) {
    out.push_str("<span class=\"");
    out.push_str(kind.class());
    out.push_str("\">");
    escape_into(out, text);
    out.push_str("</span>");
}

fn escape_into(out: &mut String, text: &str) {
    for c in text.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            _ => out.push(c),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sample_manifest_highlights_keys_and_comments() {
        let yaml = "# A supply-wagon manifest\nclaim: ridge-line cache\nseason: 2026\n";
        let painted = html(yaml);
        assert!(painted.contains("yaml-comment"));
        assert!(painted.contains("yaml-key"));
        assert!(painted.contains("yaml-number"));
        assert!(!painted.contains("yaml-doc"));
    }

    #[test]
    fn multi_doc_marks_separator_and_both_documents() {
        let yaml = "claim: ridge-line cache\n---\nschema: YamlSigilSignature.v1alpha1\n";
        let painted = html(yaml);
        assert!(painted.contains("class=\"yaml-doc\">---</span>"));
        assert!(painted.contains("claim"));
        assert!(painted.contains("schema"));
        let key_count = painted.matches("yaml-key").count();
        assert!(key_count >= 2, "{painted}");
    }

    #[test]
    fn quoted_marker_is_a_string() {
        let yaml = "note: \"---\"\n";
        let painted = html(yaml);
        assert!(painted.contains("yaml-string"));
        assert!(!painted.contains("yaml-doc"));
    }

    #[test]
    fn quoted_and_plain_scalars_share_string_class() {
        let quoted = html("note: \"ridge-line cache\"\n");
        let plain = html("note: ridge-line cache\n");
        assert!(quoted.contains("yaml-string"));
        assert!(plain.contains("yaml-string"));
        assert!(plain.contains("ridge-line"));
    }

    #[test]
    fn values_are_html_escaped() {
        let yaml = "note: a < b & c\n";
        let painted = html(yaml);
        assert!(painted.contains("&lt;"));
        assert!(painted.contains("&amp;"));
        assert!(!painted.contains("a < b"));
    }
}
