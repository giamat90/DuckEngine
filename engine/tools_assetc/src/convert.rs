//! Source image bytes in, `dtex` bytes out.

use crate::{ConvertError, mips, pnm};
use assets::dtex::{TextureSpec, encode_into, encoded_len, max_mip_count};

/// What the user asked for besides the input and output paths.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Options {
    /// Mark the colour channels as sRGB encoded.
    pub srgb: bool,
    /// Generate the full mip chain (otherwise only mip 0).
    pub mips: bool,
}

/// Converts a PPM or PAM image to `dtex` bytes.
///
/// # Errors
/// The source could not be read ([`ConvertError`]) or the writer refused the result.
pub fn convert(source: &[u8], options: Options) -> Result<Vec<u8>, ConvertError> {
    let image = pnm::parse(source)?;
    let spec = TextureSpec {
        width: image.width,
        height: image.height,
        srgb: options.srgb,
    };
    let count = if options.mips {
        max_mip_count(image.width, image.height)
    } else {
        1
    };
    let chain = mips::build_chain(image.rgba, (image.width, image.height), count);
    let levels: Vec<&[u8]> = chain.iter().map(Vec::as_slice).collect();
    let mut out = vec![0u8; encoded_len(spec, count)?];
    encode_into(&mut out, spec, &levels)?;
    Ok(out)
}

#[cfg(test)]
mod tests;
