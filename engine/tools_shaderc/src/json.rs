//! A small JSON reader, enough for the reflection output of SPIRV-Cross (card 3-11). No third-party
//! crate: the tool needs objects, arrays, strings and unsigned integers, nothing else.
//!
//! Input is limited in size and nesting, so hostile text cannot exhaust memory or the stack.
//! Numbers keep their text and are read as unsigned integers on request. The first of two equal
//! keys in an object wins.

use core::fmt;

/// Longest text accepted, in bytes.
pub const MAX_INPUT_LEN: usize = 1 << 20;
/// Deepest nesting of arrays and objects accepted.
pub const MAX_DEPTH: usize = 32;

/// Why a text was refused.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum JsonError {
    /// Not valid JSON.
    Syntax,
    /// Nested deeper than [`MAX_DEPTH`].
    TooDeep,
    /// Longer than [`MAX_INPUT_LEN`].
    TooLarge,
    /// A bad escape in a string.
    BadEscape,
}

impl fmt::Display for JsonError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Syntax => f.write_str("invalid JSON"),
            Self::TooDeep => f.write_str("JSON nested too deeply"),
            Self::TooLarge => f.write_str("JSON text too large"),
            Self::BadEscape => f.write_str("bad escape in a JSON string"),
        }
    }
}

impl std::error::Error for JsonError {}

/// A parsed JSON value.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Json {
    /// `null`.
    Null,
    /// `true` or `false`.
    Bool(bool),
    /// A number, as written.
    Number(String),
    /// A string, escapes resolved.
    String(String),
    /// An array.
    Array(Vec<Json>),
    /// An object, members in file order.
    Object(Vec<(String, Json)>),
}

impl Json {
    /// The member `key` of an object (the first, if repeated); `None` for other values.
    #[must_use]
    pub fn get(&self, key: &str) -> Option<&Json> {
        match self {
            Self::Object(members) => members.iter().find(|(k, _)| k == key).map(|(_, v)| v),
            _ => None,
        }
    }

    /// The elements of an array.
    #[must_use]
    pub fn as_array(&self) -> Option<&[Json]> {
        match self {
            Self::Array(items) => Some(items),
            _ => None,
        }
    }

    /// The text of a string.
    #[must_use]
    pub fn as_str(&self) -> Option<&str> {
        match self {
            Self::String(text) => Some(text),
            _ => None,
        }
    }

    /// A number written as plain digits, as an unsigned integer.
    #[must_use]
    pub fn as_u64(&self) -> Option<u64> {
        match self {
            Self::Number(text) => text.parse().ok(),
            _ => None,
        }
    }
}

/// Parses a complete JSON text.
///
/// # Errors
/// [`JsonError`] for text that is not JSON, is too large or nests too deeply.
pub fn parse(text: &str) -> Result<Json, JsonError> {
    if text.len() > MAX_INPUT_LEN {
        return Err(JsonError::TooLarge);
    }
    let mut parser = Parser {
        bytes: text.as_bytes(),
        pos: 0,
    };
    let value = parser.value(0)?;
    parser.skip_space();
    if parser.pos == parser.bytes.len() {
        Ok(value)
    } else {
        Err(JsonError::Syntax)
    }
}

struct Parser<'a> {
    bytes: &'a [u8],
    pos: usize,
}

impl Parser<'_> {
    fn peek(&self) -> Option<u8> {
        self.bytes.get(self.pos).copied()
    }

    fn skip_space(&mut self) {
        while matches!(self.peek(), Some(b' ' | b'\t' | b'\n' | b'\r')) {
            self.pos += 1;
        }
    }

    fn eat(&mut self, byte: u8) -> Result<(), JsonError> {
        if self.peek() == Some(byte) {
            self.pos += 1;
            Ok(())
        } else {
            Err(JsonError::Syntax)
        }
    }

    fn literal(&mut self, word: &str, value: Json) -> Result<Json, JsonError> {
        let end = self.pos + word.len();
        if self.bytes.get(self.pos..end) == Some(word.as_bytes()) {
            self.pos = end;
            Ok(value)
        } else {
            Err(JsonError::Syntax)
        }
    }

    fn value(&mut self, depth: usize) -> Result<Json, JsonError> {
        self.skip_space();
        match self.peek().ok_or(JsonError::Syntax)? {
            b'{' => self.object(depth),
            b'[' => self.array(depth),
            b'"' => self.string().map(Json::String),
            b't' => self.literal("true", Json::Bool(true)),
            b'f' => self.literal("false", Json::Bool(false)),
            b'n' => self.literal("null", Json::Null),
            _ => self.number(),
        }
    }

    fn enter(depth: usize) -> Result<usize, JsonError> {
        if depth >= MAX_DEPTH {
            Err(JsonError::TooDeep)
        } else {
            Ok(depth + 1)
        }
    }

    fn array(&mut self, depth: usize) -> Result<Json, JsonError> {
        let depth = Self::enter(depth)?;
        self.eat(b'[')?;
        let mut items = Vec::new();
        self.skip_space();
        if self.peek() == Some(b']') {
            self.pos += 1;
            return Ok(Json::Array(items));
        }
        let mut done = false;
        while !done {
            items.push(self.value(depth)?);
            self.skip_space();
            done = self.separator(b']')?;
        }
        Ok(Json::Array(items))
    }

    /// After an element: `,` means more follow (`false`), the closing byte ends the container
    /// (`true`).
    fn separator(&mut self, close: u8) -> Result<bool, JsonError> {
        match self.peek() {
            Some(b',') => {
                self.pos += 1;
                Ok(false)
            }
            Some(byte) if byte == close => {
                self.pos += 1;
                Ok(true)
            }
            _ => Err(JsonError::Syntax),
        }
    }

    fn object(&mut self, depth: usize) -> Result<Json, JsonError> {
        let depth = Self::enter(depth)?;
        self.eat(b'{')?;
        let mut members = Vec::new();
        self.skip_space();
        if self.peek() == Some(b'}') {
            self.pos += 1;
            return Ok(Json::Object(members));
        }
        let mut done = false;
        while !done {
            members.push(self.member(depth)?);
            self.skip_space();
            done = self.separator(b'}')?;
        }
        Ok(Json::Object(members))
    }

    fn member(&mut self, depth: usize) -> Result<(String, Json), JsonError> {
        self.skip_space();
        let key = self.string()?;
        self.skip_space();
        self.eat(b':')?;
        Ok((key, self.value(depth)?))
    }

    fn hex4(&mut self) -> Result<u32, JsonError> {
        let digits = self
            .bytes
            .get(self.pos..self.pos + 4)
            .ok_or(JsonError::BadEscape)?;
        let text = core::str::from_utf8(digits).map_err(|_| JsonError::BadEscape)?;
        if !text.bytes().all(|b| b.is_ascii_hexdigit()) {
            return Err(JsonError::BadEscape);
        }
        self.pos += 4;
        u32::from_str_radix(text, 16).map_err(|_| JsonError::BadEscape)
    }

    fn escape(&mut self) -> Result<char, JsonError> {
        let byte = self.peek().ok_or(JsonError::BadEscape)?;
        self.pos += 1;
        match byte {
            b'"' => Ok('"'),
            b'\\' => Ok('\\'),
            b'/' => Ok('/'),
            b'b' => Ok('\u{8}'),
            b'f' => Ok('\u{c}'),
            b'n' => Ok('\n'),
            b'r' => Ok('\r'),
            b't' => Ok('\t'),
            b'u' => char::from_u32(self.hex4()?).ok_or(JsonError::BadEscape),
            _ => Err(JsonError::BadEscape),
        }
    }

    fn push_escape(&mut self, out: &mut Vec<u8>) -> Result<(), JsonError> {
        let c = self.escape()?;
        out.extend_from_slice(c.encode_utf8(&mut [0u8; 4]).as_bytes());
        Ok(())
    }

    fn string(&mut self) -> Result<String, JsonError> {
        self.eat(b'"')?;
        let mut out = Vec::new();
        loop {
            let byte = self.peek().ok_or(JsonError::Syntax)?;
            self.pos += 1;
            match byte {
                b'"' => break,
                b'\\' => self.push_escape(&mut out)?,
                0..=0x1f => return Err(JsonError::Syntax),
                other => out.push(other),
            }
        }
        String::from_utf8(out).map_err(|_| JsonError::Syntax)
    }

    fn digits(&mut self) -> usize {
        let start = self.pos;
        while matches!(self.peek(), Some(b'0'..=b'9')) {
            self.pos += 1;
        }
        self.pos - start
    }

    fn require_digits(&mut self) -> Result<(), JsonError> {
        if self.digits() == 0 {
            Err(JsonError::Syntax)
        } else {
            Ok(())
        }
    }

    fn fraction(&mut self) -> Result<(), JsonError> {
        if self.peek() != Some(b'.') {
            return Ok(());
        }
        self.pos += 1;
        self.require_digits()
    }

    fn exponent(&mut self) -> Result<(), JsonError> {
        if !matches!(self.peek(), Some(b'e' | b'E')) {
            return Ok(());
        }
        self.pos += 1;
        if matches!(self.peek(), Some(b'+' | b'-')) {
            self.pos += 1;
        }
        self.require_digits()
    }

    fn number(&mut self) -> Result<Json, JsonError> {
        let start = self.pos;
        if self.peek() == Some(b'-') {
            self.pos += 1;
        }
        let leading_zero = self.peek() == Some(b'0');
        let int_digits = self.digits();
        if int_digits == 0 || (leading_zero && int_digits > 1) {
            return Err(JsonError::Syntax);
        }
        self.fraction()?;
        self.exponent()?;
        let text =
            core::str::from_utf8(&self.bytes[start..self.pos]).map_err(|_| JsonError::Syntax)?;
        Ok(Json::Number(text.to_string()))
    }
}

#[cfg(test)]
mod tests;
