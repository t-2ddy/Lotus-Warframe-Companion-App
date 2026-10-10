//! Finds where a JSON object ends inside raw memory, without parsing it.

/// Result of scanning a buffer that starts with `{`.
#[derive(Debug, PartialEq, Eq)]
pub enum ObjectEnd {
    /// The object closes at this index (exclusive).
    Complete(usize),
    /// The buffer ended before the object closed; read more and try again.
    Incomplete,
    /// The bytes are not a clean JSON object (garbage, or memory was overwritten) at this index.
    Invalid(usize),
}

/// Walks a buffer that should start with a JSON object and returns where it ends.
///
/// Strings and escapes are respected, so braces inside strings don't count.
/// Bytes that can't appear in JSON (control characters, non-ASCII outside strings)
/// mark the candidate as invalid, which is how overwritten copies get rejected.
pub fn object_end(buf: &[u8]) -> ObjectEnd {
    if buf.first() != Some(&b'{') {
        return ObjectEnd::Invalid(0);
    }
    let mut depth = 0usize;
    let mut in_str = false;
    let mut escaped = false;
    for (i, &c) in buf.iter().enumerate() {
        if in_str {
            if escaped {
                escaped = false;
            } else if c == b'\\' {
                escaped = true;
            } else if c == b'"' {
                in_str = false;
            } else if c < 0x20 {
                return ObjectEnd::Invalid(i);
            }
            continue;
        }
        match c {
            b'"' => in_str = true,
            b'{' | b'[' => depth += 1,
            b'}' | b']' => {
                if depth == 0 {
                    return ObjectEnd::Invalid(i);
                }
                depth -= 1;
                if depth == 0 {
                    return ObjectEnd::Complete(i + 1);
                }
            }
            b' ' | b'\t' | b'\n' | b'\r' => {}
            c if c < 0x20 || c >= 0x80 => return ObjectEnd::Invalid(i),
            _ => {}
        }
    }
    ObjectEnd::Incomplete
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_end_of_nested_object() {
        let buf = br#"{"a":[1,{"b":"}"}],"c":"x\"}"}trailing"#;
        assert_eq!(object_end(buf), ObjectEnd::Complete(buf.len() - "trailing".len()));
    }

    #[test]
    fn incomplete_when_buffer_runs_out() {
        assert_eq!(object_end(br#"{"a":[1,2"#), ObjectEnd::Incomplete);
    }

    #[test]
    fn invalid_on_garbage() {
        assert_eq!(object_end(b"{\"a\":\x00\x01}"), ObjectEnd::Invalid(5));
        assert_eq!(object_end(b"{\"a\":1\xff}"), ObjectEnd::Invalid(6));
        assert_eq!(object_end(b"not json"), ObjectEnd::Invalid(0));
    }

    #[test]
    fn utf8_inside_strings_is_fine() {
        let buf = "{\"name\":\"Ash Prime \u{2014} Neuroptics\"}".as_bytes();
        assert_eq!(object_end(buf), ObjectEnd::Complete(buf.len()));
    }
}
