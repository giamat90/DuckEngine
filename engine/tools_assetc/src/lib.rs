//! `assetc`, the offline asset converter (tier T2, dev-time only): source images in, `dtex` out.
//!
//! The shipped runtime never links this crate. Source formats in this version: binary PPM (`P6`)
//! and PAM (`P7`, RGB or `RGB_ALPHA`, 8 bits). PNG needs an image-decoding crate and a
//! `THIRD_PARTY.md` row, which is a separate governance step (design note section 8, O5).

pub mod cli;
pub mod convert;
pub mod mips;
pub mod pnm;

use assets::dtex::DtexError;
use core::fmt;

/// Why a conversion failed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ConvertError {
    /// The input is not a format this tool reads.
    UnknownFormat,
    /// The header of the source file is malformed.
    BadHeader,
    /// A valid header that asks for something not supported (bit depth, channel count).
    Unsupported,
    /// Width or height is zero or too large.
    BadDimensions,
    /// The pixel data length does not match the header.
    BadPixelData,
    /// The `dtex` writer refused the result.
    Dtex(DtexError),
}

impl fmt::Display for ConvertError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnknownFormat => f.write_str("unknown source format (supported: PPM P6, PAM P7)"),
            Self::BadHeader => f.write_str("malformed image header"),
            Self::Unsupported => f.write_str("unsupported image (only 8 bits, RGB or RGBA)"),
            Self::BadDimensions => f.write_str("image width or height is zero or too large"),
            Self::BadPixelData => f.write_str("pixel data length does not match the header"),
            Self::Dtex(e) => write!(f, "dtex: {e}"),
        }
    }
}

impl std::error::Error for ConvertError {}

impl From<DtexError> for ConvertError {
    fn from(e: DtexError) -> Self {
        Self::Dtex(e)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_error_has_its_own_message() {
        let all = [
            ConvertError::UnknownFormat,
            ConvertError::BadHeader,
            ConvertError::Unsupported,
            ConvertError::BadDimensions,
            ConvertError::BadPixelData,
            ConvertError::Dtex(DtexError::Truncated),
        ];
        let texts: Vec<String> = all.iter().map(ToString::to_string).collect();
        for (i, a) in texts.iter().enumerate() {
            assert!(!a.is_empty());
            assert!(texts[i + 1..].iter().all(|b| a != b));
        }
        assert!(texts[5].contains("truncated"));
        assert_eq!(
            ConvertError::from(DtexError::BadMagic),
            ConvertError::Dtex(DtexError::BadMagic)
        );
    }
}
