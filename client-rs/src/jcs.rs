//! RFC 8785 — JSON Canonicalization Scheme.
//!
//! The signature over an Agent Card is computed across canonical bytes, so this
//! has to agree with the vendor side exactly: a mismatch here does not produce a
//! wrong answer, it produces "signature invalid" on a perfectly good card.
//!
//! Two details that are easy to get wrong and are the usual cause of that:
//! keys sort by **UTF-16 code unit**, not by UTF-8 byte or Unicode scalar, and
//! the solidus `/` is **not** escaped.

use serde_json::Value;

pub fn canonicalize(v: &Value) -> Result<Vec<u8>, String> {
    let mut out = String::new();
    write_value(v, &mut out)?;
    Ok(out.into_bytes())
}

fn write_string(s: &str, out: &mut String) {
    out.push('"');
    for ch in s.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\u{08}' => out.push_str("\\b"),
            '\u{09}' => out.push_str("\\t"),
            '\u{0A}' => out.push_str("\\n"),
            '\u{0C}' => out.push_str("\\f"),
            '\u{0D}' => out.push_str("\\r"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
}

/// UTF-16 code units, per RFC 8785 §3.2.3.
fn utf16_key(s: &str) -> Vec<u16> {
    s.encode_utf16().collect()
}

fn write_value(v: &Value, out: &mut String) -> Result<(), String> {
    match v {
        Value::Null => out.push_str("null"),
        Value::Bool(true) => out.push_str("true"),
        Value::Bool(false) => out.push_str("false"),
        Value::String(s) => write_string(s, out),
        Value::Number(n) => {
            // Floats would need ECMAScript Number::toString semantics. Nothing
            // in an Agent Card needs one, so refusing is safer than emitting
            // subtly non-canonical bytes that fail verification later.
            let i = n.as_i64().ok_or("JCS: only integers are supported here")?;
            out.push_str(&i.to_string());
        }
        Value::Array(a) => {
            out.push('[');
            for (i, item) in a.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                write_value(item, out)?;
            }
            out.push(']');
        }
        Value::Object(m) => {
            let mut keys: Vec<&String> = m.keys().collect();
            keys.sort_by(|a, b| utf16_key(a).cmp(&utf16_key(b)));
            out.push('{');
            for (i, k) in keys.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                write_string(k, out);
                out.push(':');
                write_value(&m[*k], out)?;
            }
            out.push('}');
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn keys_sort_and_output_is_compact() {
        let v = json!({"b": 1, "a": 2});
        assert_eq!(canonicalize(&v).unwrap(), b"{\"a\":2,\"b\":1}");
    }

    #[test]
    fn utf16_ordering() {
        // 'a' U+0061 sorts before 'ä' U+00E4 by UTF-16 code unit.
        let v = json!({"ä": 1, "a": 2});
        let out = String::from_utf8(canonicalize(&v).unwrap()).unwrap();
        assert!(out.starts_with("{\"a\":2"), "got {out}");
    }

    #[test]
    fn solidus_is_not_escaped() {
        let v = json!({"a": "/"});
        assert_eq!(canonicalize(&v).unwrap(), b"{\"a\":\"/\"}");
    }

    #[test]
    fn control_characters_use_short_escapes() {
        let v = json!({"a": "x\ty\n"});
        assert_eq!(canonicalize(&v).unwrap(), b"{\"a\":\"x\\ty\\n\"}");
    }

    #[test]
    fn floats_are_refused_rather_than_approximated() {
        assert!(canonicalize(&json!({"a": 1.5})).is_err());
    }
}
