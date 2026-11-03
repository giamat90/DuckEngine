//! Reader for binary PPM (`P6`) and PAM (`P7`) images, 8 bits per channel, RGB or RGBA.

use crate::ConvertError;
use assets::dtex::MAX_DIMENSION;

/// A decoded image: RGBA8, row by row from the top.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Image {
    /// Width in pixels.
    pub width: u32,
    /// Height in pixels.
    pub height: u32,
    /// `width * height * 4` bytes.
    pub rgba: Vec<u8>,
}

/// Reads whitespace and `#` comment separated tokens from a byte slice.
struct Tokens<'a> {
    bytes: &'a [u8],
    pos: usize,
}

impl<'a> Tokens<'a> {
    fn skip_blank(&mut self) {
        while let Some(&b) = self.bytes.get(self.pos) {
            match b {
                b'#' => self.skip_comment(),
                _ if b.is_ascii_whitespace() => self.pos += 1,
                _ => break,
            }
        }
    }

    fn skip_comment(&mut self) {
        while self.bytes.get(self.pos).is_some_and(|&c| c != b'\n') {
            self.pos += 1;
        }
    }

    /// The next token, ended by whitespace or the end of the data.
    fn token(&mut self) -> Option<&'a [u8]> {
        self.skip_blank();
        let start = self.pos;
        while self
            .bytes
            .get(self.pos)
            .is_some_and(|b| !b.is_ascii_whitespace())
        {
            self.pos += 1;
        }
        (self.pos > start).then(|| &self.bytes[start..self.pos])
    }

    fn number(&mut self) -> Option<u32> {
        let text = core::str::from_utf8(self.token()?).ok()?;
        text.parse().ok()
    }

    /// Consumes the single whitespace byte that separates the header from the pixels, and returns
    /// the rest.
    fn pixels(&mut self) -> Option<&'a [u8]> {
        self.bytes
            .get(self.pos)
            .filter(|b| b.is_ascii_whitespace())?;
        self.bytes.get(self.pos + 1..)
    }
}

/// The header fields both formats share.
struct Header {
    width: u32,
    height: u32,
    depth: usize,
    maxval: u32,
}

fn p6_header(tokens: &mut Tokens<'_>) -> Result<Header, ConvertError> {
    let width = tokens.number().ok_or(ConvertError::BadHeader)?;
    let height = tokens.number().ok_or(ConvertError::BadHeader)?;
    let maxval = tokens.number().ok_or(ConvertError::BadHeader)?;
    Ok(Header {
        width,
        height,
        depth: 3,
        maxval,
    })
}

fn p7_header(tokens: &mut Tokens<'_>) -> Result<Header, ConvertError> {
    let (mut width, mut height, mut depth, mut maxval) = (None, None, None, None);
    let mut tuple: Option<&[u8]> = None;
    loop {
        match tokens.token().ok_or(ConvertError::BadHeader)? {
            b"ENDHDR" => break,
            b"WIDTH" => width = tokens.number(),
            b"HEIGHT" => height = tokens.number(),
            b"DEPTH" => depth = tokens.number(),
            b"MAXVAL" => maxval = tokens.number(),
            b"TUPLTYPE" => tuple = Some(tokens.token().ok_or(ConvertError::BadHeader)?),
            _ => return Err(ConvertError::BadHeader),
        }
    }
    let depth = usize::try_from(depth.ok_or(ConvertError::BadHeader)?)
        .map_err(|_| ConvertError::BadHeader)?;
    // A tuple type, when given, must say the same as the depth: RGB for 3, RGB_ALPHA for 4.
    if tuple.is_some_and(|t| !matches!((depth, t), (3, b"RGB") | (4, b"RGB_ALPHA"))) {
        return Err(ConvertError::Unsupported);
    }
    Ok(Header {
        width: width.ok_or(ConvertError::BadHeader)?,
        height: height.ok_or(ConvertError::BadHeader)?,
        depth,
        maxval: maxval.ok_or(ConvertError::BadHeader)?,
    })
}

fn to_rgba(header: &Header, data: &[u8]) -> Result<Vec<u8>, ConvertError> {
    let pixel_count = usize::try_from(header.width)
        .ok()
        .zip(usize::try_from(header.height).ok());
    let pixel_count = pixel_count
        .and_then(|(w, h)| w.checked_mul(h))
        .ok_or(ConvertError::BadDimensions)?;
    let expected = pixel_count
        .checked_mul(header.depth)
        .ok_or(ConvertError::BadDimensions)?;
    if data.len() != expected {
        return Err(ConvertError::BadPixelData);
    }
    let mut rgba = Vec::with_capacity(pixel_count * 4);
    for pixel in data.chunks_exact(header.depth) {
        rgba.extend_from_slice(&pixel[..3]);
        rgba.push(pixel.get(3).copied().unwrap_or(u8::MAX));
    }
    Ok(rgba)
}

/// Decodes `bytes` as a P6 or P7 image.
///
/// # Errors
/// See [`ConvertError`]. Hostile input yields an error, never a panic or a huge allocation: the
/// pixel buffer is only built after the data length matched the header.
pub fn parse(bytes: &[u8]) -> Result<Image, ConvertError> {
    let mut tokens = Tokens { bytes, pos: 0 };
    let header = match tokens.token() {
        Some(b"P6") => p6_header(&mut tokens)?,
        Some(b"P7") => p7_header(&mut tokens)?,
        _ => return Err(ConvertError::UnknownFormat),
    };
    let in_range = |v: u32| (1..=MAX_DIMENSION).contains(&v);
    if !in_range(header.width) || !in_range(header.height) {
        return Err(ConvertError::BadDimensions);
    }
    if header.maxval != 255 || !(3..=4).contains(&header.depth) {
        return Err(ConvertError::Unsupported);
    }
    let data = tokens.pixels().ok_or(ConvertError::BadHeader)?;
    let rgba = to_rgba(&header, data)?;
    Ok(Image {
        width: header.width,
        height: header.height,
        rgba,
    })
}

#[cfg(test)]
mod tests;
