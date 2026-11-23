// SPDX-License-Identifier: MIT
//! A small JSON reader, for the three things omui reads that are JSON: a translation catalog, Omarchy's registry of
//! languages, and `hyprctl -j`. omui depends on nothing but GTK, and these are flat objects of strings and numbers,
//! so a parser of its own is less to carry than a crate every app would then build.

/// A JSON value. An object keeps its members in the order they were written, and the last of two with one name wins
/// in [`Value::get`].
#[derive(Clone, Debug, PartialEq)]
pub enum Value {
    Null,
    Bool(bool),
    Number(f64),
    String(String),
    Array(Vec<Value>),
    Object(Vec<(String, Value)>),
}

impl Value {
    /// A member of an object; None for any other value, or when there is no such member.
    pub fn get(&self, name: &str) -> Option<&Value> {
        match self {
            Value::Object(members) => members
                .iter()
                .rev()
                .find(|(key, _)| key == name)
                .map(|(_, value)| value),
            _ => None,
        }
    }

    pub fn as_str(&self) -> Option<&str> {
        match self {
            Value::String(text) => Some(text),
            _ => None,
        }
    }

    pub fn as_f64(&self) -> Option<f64> {
        match self {
            Value::Number(number) => Some(*number),
            _ => None,
        }
    }

    pub fn members(&self) -> &[(String, Value)] {
        match self {
            Value::Object(members) => members,
            _ => &[],
        }
    }
}

/// Reads one JSON value, which must be the whole of `source`. The error says where reading stopped.
pub fn parse(source: &str) -> Result<Value, String> {
    let mut reader = Reader {
        bytes: source.as_bytes(),
        at: 0,
        depth: 0,
    };
    let value = reader.value()?;
    reader.space();
    if reader.at < reader.bytes.len() {
        return Err(reader.error("something follows the value"));
    }
    Ok(value)
}

/// Deeper than any file omui reads; a file nested further is refused rather than overflowing the stack.
const MAX_DEPTH: usize = 64;

struct Reader<'a> {
    bytes: &'a [u8],
    at: usize,
    depth: usize,
}

impl Reader<'_> {
    fn error(&self, what: &str) -> String {
        format!("JSON: {what} at byte {}", self.at)
    }

    fn space(&mut self) {
        while matches!(self.bytes.get(self.at), Some(b' ' | b'\t' | b'\n' | b'\r')) {
            self.at += 1;
        }
    }

    fn take(&mut self, byte: u8) -> bool {
        self.space();
        if self.bytes.get(self.at) == Some(&byte) {
            self.at += 1;
            true
        } else {
            false
        }
    }

    fn word(&mut self, word: &str, value: Value) -> Result<Value, String> {
        if self.bytes[self.at..].starts_with(word.as_bytes()) {
            self.at += word.len();
            Ok(value)
        } else {
            Err(self.error("an unknown word"))
        }
    }

    fn value(&mut self) -> Result<Value, String> {
        self.space();
        match self.bytes.get(self.at) {
            Some(b'{') => self.nested(Self::object),
            Some(b'[') => self.nested(Self::array),
            Some(b'"') => self.string().map(Value::String),
            Some(b't') => self.word("true", Value::Bool(true)),
            Some(b'f') => self.word("false", Value::Bool(false)),
            Some(b'n') => self.word("null", Value::Null),
            Some(b'-' | b'0'..=b'9') => self.number(),
            Some(_) => Err(self.error("not a value")),
            None => Err(self.error("the text ends where a value should be")),
        }
    }

    fn nested(&mut self, read: fn(&mut Self) -> Result<Value, String>) -> Result<Value, String> {
        if self.depth == MAX_DEPTH {
            return Err(self.error("nested too deeply"));
        }
        self.depth += 1;
        self.at += 1;
        let value = read(self);
        self.depth -= 1;
        value
    }

    fn object(&mut self) -> Result<Value, String> {
        let mut members = Vec::new();
        if self.take(b'}') {
            return Ok(Value::Object(members));
        }
        loop {
            self.space();
            if self.bytes.get(self.at) != Some(&b'"') {
                return Err(self.error("a member needs a name in quotes"));
            }
            let name = self.string()?;
            if !self.take(b':') {
                return Err(self.error("a member needs a colon after its name"));
            }
            members.push((name, self.value()?));
            if self.take(b'}') {
                return Ok(Value::Object(members));
            }
            if !self.take(b',') {
                return Err(self.error("an object's members are separated by commas"));
            }
        }
    }

    fn array(&mut self) -> Result<Value, String> {
        let mut items = Vec::new();
        if self.take(b']') {
            return Ok(Value::Array(items));
        }
        loop {
            items.push(self.value()?);
            if self.take(b']') {
                return Ok(Value::Array(items));
            }
            if !self.take(b',') {
                return Err(self.error("an array's items are separated by commas"));
            }
        }
    }

    fn number(&mut self) -> Result<Value, String> {
        let start = self.at;
        while matches!(
            self.bytes.get(self.at),
            Some(b'-' | b'+' | b'.' | b'e' | b'E' | b'0'..=b'9')
        ) {
            self.at += 1;
        }
        // The bytes are ASCII, so they are a string.
        std::str::from_utf8(&self.bytes[start..self.at])
            .ok()
            .and_then(|text| text.parse().ok())
            .map(Value::Number)
            .ok_or_else(|| self.error("not a number"))
    }

    /// Four hex digits of a `\u` escape.
    fn unit(&mut self) -> Result<u32, String> {
        let digits = self
            .bytes
            .get(self.at..self.at + 4)
            .and_then(|digits| std::str::from_utf8(digits).ok())
            .and_then(|digits| u32::from_str_radix(digits, 16).ok())
            .ok_or_else(|| self.error("a \\u escape needs four hex digits"))?;
        self.at += 4;
        Ok(digits)
    }

    fn string(&mut self) -> Result<String, String> {
        self.at += 1;
        let mut text = Vec::new();
        loop {
            let Some(&byte) = self.bytes.get(self.at) else {
                return Err(self.error("the text ends inside a string"));
            };
            self.at += 1;
            match byte {
                b'"' => break,
                b'\\' => {
                    let Some(&escaped) = self.bytes.get(self.at) else {
                        return Err(self.error("the text ends inside a string"));
                    };
                    self.at += 1;
                    let character = match escaped {
                        b'"' | b'\\' | b'/' => char::from(escaped),
                        b'n' => '\n',
                        b't' => '\t',
                        b'r' => '\r',
                        b'b' => '\u{8}',
                        b'f' => '\u{c}',
                        b'u' => {
                            let mut code = self.unit()?;
                            // A character outside the first plane is written as two escapes, a surrogate pair.
                            if (0xd800..0xdc00).contains(&code) && self.bytes[self.at..].starts_with(b"\\u") {
                                self.at += 2;
                                let low = self.unit()?;
                                code = 0x10000 + ((code - 0xd800) << 10) + (low.wrapping_sub(0xdc00) & 0x3ff);
                            }
                            char::from_u32(code).unwrap_or(char::REPLACEMENT_CHARACTER)
                        }
                        _ => return Err(self.error("an unknown escape")),
                    };
                    text.extend_from_slice(character.encode_utf8(&mut [0; 4]).as_bytes());
                }
                _ => text.push(byte),
            }
        }
        // The source was a string and escapes add whole characters, so the bytes between two quotes are one too.
        String::from_utf8(text).map_err(|_| self.error("a string that is not UTF-8"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_catalog_and_hyprctls_answer_are_read() {
        let catalog = parse(
            "{\n  \"Save\": \"Gem\",\n  \"{n} clips\": { \"one\": \"{n} klip\", \"other\": \"{n} klip\" },\n  \
             \"Quote \\\"it\\\"\": \"Citér \\u00e9 \\ud83d\\ude00\\n\"\n}",
        )
        .unwrap();
        assert_eq!(catalog.get("Save").and_then(Value::as_str), Some("Gem"));
        assert_eq!(
            catalog
                .get("{n} clips")
                .and_then(|forms| forms.get("one"))
                .and_then(Value::as_str),
            Some("{n} klip")
        );
        assert_eq!(
            catalog.get("Quote \"it\"").and_then(Value::as_str),
            Some("Citér é 😀\n")
        );
        assert_eq!(catalog.members().len(), 3);
        let answer = parse(r#"{"option": "decoration:rounding", "int": 4, "set": true }"#).unwrap();
        assert_eq!(answer.get("int").and_then(Value::as_f64), Some(4.0));
        assert_eq!(answer.get("set"), Some(&Value::Bool(true)));
    }

    #[test]
    fn every_kind_of_value_is_read_and_what_is_not_json_is_refused() {
        assert_eq!(
            parse(" [1, -2.5e1, null, false, [], {}] ").unwrap(),
            Value::Array(vec![
                Value::Number(1.0),
                Value::Number(-25.0),
                Value::Null,
                Value::Bool(false),
                Value::Array(Vec::new()),
                Value::Object(Vec::new()),
            ])
        );
        for broken in [
            "",
            "{",
            "{\"a\" 1}",
            "{\"a\": 1,}",
            "[1 2]",
            "\"open",
            "tru",
            "{} x",
            "\"\\q\"",
            "1.2.3",
        ] {
            assert!(parse(broken).is_err(), "{broken:?}");
        }
        assert!(
            parse(&"[".repeat(10_000)).is_err(),
            "deep nesting is refused, not followed"
        );
        // The later of two members with one name is the one read.
        assert_eq!(
            parse(r#"{"a": 1, "a": 2}"#).unwrap().get("a"),
            Some(&Value::Number(2.0))
        );
    }
}
