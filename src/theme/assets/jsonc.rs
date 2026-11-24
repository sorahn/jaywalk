//! Keep byte offsets/newlines for useful serde diagnostics while stripping JSONC.
pub(super) fn parse(text: &str) -> Result<serde_json::Value, String> {
    let mut bytes = text.as_bytes().to_vec();
    if bytes.starts_with(&[0xef, 0xbb, 0xbf]) {
        bytes[..3].fill(b' ');
    }
    let (mut i, mut quoted, mut escaped) = (0, false, false);
    while i < bytes.len() {
        let b = bytes[i];
        if quoted {
            if escaped {
                escaped = false;
            } else if b == b'\\' {
                escaped = true;
            } else if b == b'"' {
                quoted = false;
            }
            i += 1;
            continue;
        }
        if b == b'"' {
            quoted = true;
            i += 1;
            continue;
        }
        if b == b'/' && bytes.get(i + 1) == Some(&b'/') {
            while i < bytes.len() && bytes[i] != b'\n' {
                bytes[i] = b' ';
                i += 1;
            }
        } else if b == b'/' && bytes.get(i + 1) == Some(&b'*') {
            bytes[i] = b' ';
            bytes[i + 1] = b' ';
            i += 2;
            loop {
                if i + 1 >= bytes.len() {
                    return Err("Unterminated JSONC block comment".into());
                }
                if bytes[i] == b'*' && bytes[i + 1] == b'/' {
                    bytes[i] = b' ';
                    bytes[i + 1] = b' ';
                    i += 2;
                    break;
                }
                if bytes[i] != b'\n' && bytes[i] != b'\r' {
                    bytes[i] = b' ';
                }
                i += 1;
            }
        } else {
            i += 1;
        }
    }
    // Remove commas immediately before a closing array/object, outside strings.
    let (mut quoted, mut escaped) = (false, false);
    for i in 0..bytes.len() {
        let b = bytes[i];
        if quoted {
            if escaped {
                escaped = false;
            } else if b == b'\\' {
                escaped = true;
            } else if b == b'"' {
                quoted = false;
            }
        } else if b == b'"' {
            quoted = true;
        } else if b == b','
            && bytes[i + 1..]
                .iter()
                .find(|b| !b.is_ascii_whitespace())
                .is_some_and(|b| matches!(b, b']' | b'}'))
        {
            bytes[i] = b' ';
        }
    }
    serde_json::from_slice(&bytes).map_err(|e| e.to_string())
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn comments_and_commas_do_not_change_strings_or_unicode() {
        let v = parse("{ // hi\n\"name\": \"héllo // /* x */ ,} \\\"\", /* ok */ \"a\": [1,],}")
            .unwrap();
        assert_eq!(v["a"], serde_json::json!([1]));
        assert_eq!(v["name"], "héllo // /* x */ ,} \"");
        assert!(parse("{/*").is_err());
        assert!(parse("{\"a\":1,,}").is_err());
    }
}
