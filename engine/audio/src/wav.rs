//! The WAV decoder: RIFF/WAVE bytes in, an immutable [`Sound`] out (card 5-4).
//!
//! Reads PCM at 8 (unsigned), 16, 24 and 32 bits and IEEE float at 32 bits, mono or multichannel
//! up to [`MAX_SOUND_CHANNELS`], in the plain `fmt ` form and the extensible form. Unknown chunks
//! are skipped. Everything else (ADPCM and other compressions, 64-bit float, odd bit depths, more
//! channels) is refused with [`DecodeError::Unsupported`].
//!
//! The input is untrusted. Every size is checked against the bytes that exist before it is used,
//! all arithmetic is checked, nothing is indexed without a bounds check, the decoded size has a
//! cap that is tested before anything is allocated, and there is no `unsafe`. The RIFF size field
//! is not trusted (many files carry a wrong one); the chunk sizes are, but only up to the end of
//! the input. A `data` chunk that announces more bytes than the file holds is
//! [`DecodeError::Truncated`]. The loop over chunks advances by at least eight bytes per round, so
//! it ends within `len / 8` rounds.

use crate::decoder::{DecodeError, IAudioDecoder};
use crate::sound::{MAX_SOUND_CHANNELS, Sound};

/// The default cap on the decoded samples of one file: 64 Mi samples, 256 MiB as `f32`.
pub const DEFAULT_MAX_SAMPLES: usize = 64 * 1024 * 1024;

const RIFF_HEADER_LEN: usize = 12;
const CHUNK_HEADER_LEN: usize = 8;
const FMT_CORE_LEN: usize = 16;
const FMT_EXTENSIBLE_LEN: usize = 40;
const TAG_PCM: u16 = 1;
const TAG_FLOAT: u16 = 3;
const TAG_EXTENSIBLE: u16 = 0xFFFE;
/// The tail of the standard sub-format GUID, after the two bytes of the format tag.
const GUID_TAIL: [u8; 14] = [
    0x00, 0x00, 0x00, 0x00, 0x10, 0x00, 0x80, 0x00, 0x00, 0xAA, 0x00, 0x38, 0x9B, 0x71,
];
/// The largest absolute value of a float sample the decoder accepts: far above any real
/// headroom, far below the point where mixing could overflow to infinity.
pub const MAX_FLOAT_MAGNITUDE: f32 = 1.0e6;
const SCALE_8: f32 = 128.0;
const SCALE_16: f32 = 32_768.0;
const SCALE_24: f64 = 8_388_608.0;
const SCALE_32: f64 = 2_147_483_648.0;

/// The WAV decoder; holds only its size cap.
#[derive(Clone, Copy, Debug)]
pub struct WavDecoder {
    max_samples: usize,
}

impl WavDecoder {
    /// A decoder with the default cap, [`DEFAULT_MAX_SAMPLES`].
    #[must_use]
    pub fn new() -> Self {
        Self::with_max_samples(DEFAULT_MAX_SAMPLES)
    }

    /// A decoder that refuses files with more than `max_samples` samples (all channels counted)
    /// with [`DecodeError::TooLarge`].
    #[must_use]
    pub fn with_max_samples(max_samples: usize) -> Self {
        Self { max_samples }
    }
}

impl Default for WavDecoder {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Encoding {
    Pcm,
    Float,
}

/// What the `fmt ` chunk says, validated.
#[derive(Clone, Copy, Debug)]
struct Format {
    encoding: Encoding,
    channels: u8,
    sample_rate: u32,
    bytes_per_sample: usize,
}

fn slice(bytes: &[u8], start: usize, len: usize) -> Result<&[u8], DecodeError> {
    let end = start.checked_add(len).ok_or(DecodeError::Truncated)?;
    bytes.get(start..end).ok_or(DecodeError::Truncated)
}

fn le_u16(bytes: &[u8], at: usize) -> Result<u16, DecodeError> {
    match slice(bytes, at, 2)? {
        &[a, b] => Ok(u16::from_le_bytes([a, b])),
        _ => Err(DecodeError::Truncated),
    }
}

fn le_u32(bytes: &[u8], at: usize) -> Result<u32, DecodeError> {
    match slice(bytes, at, 4)? {
        &[a, b, c, d] => Ok(u32::from_le_bytes([a, b, c, d])),
        _ => Err(DecodeError::Truncated),
    }
}

/// Checks the extensible part of the `fmt ` chunk and returns the real format tag.
fn extensible_tag(body: &[u8], container_bits: u16) -> Result<u16, DecodeError> {
    if body.len() < FMT_EXTENSIBLE_LEN {
        return Err(DecodeError::Invalid);
    }
    if le_u16(body, 18)? > container_bits {
        return Err(DecodeError::Invalid);
    }
    if slice(body, 26, GUID_TAIL.len())? != GUID_TAIL {
        return Err(DecodeError::Unsupported);
    }
    le_u16(body, 24)
}

fn bytes_per_sample(encoding: Encoding, bits: u16) -> Result<usize, DecodeError> {
    match (encoding, bits) {
        (_, 0) => Err(DecodeError::Invalid),
        (Encoding::Pcm, 8) => Ok(1),
        (Encoding::Pcm, 16) => Ok(2),
        (Encoding::Pcm, 24) => Ok(3),
        (Encoding::Pcm | Encoding::Float, 32) => Ok(4),
        _ => Err(DecodeError::Unsupported),
    }
}

fn parse_fmt(body: &[u8]) -> Result<Format, DecodeError> {
    if body.len() < FMT_CORE_LEN {
        return Err(DecodeError::Invalid);
    }
    let bits = le_u16(body, 14)?;
    let mut tag = le_u16(body, 0)?;
    if tag == TAG_EXTENSIBLE {
        tag = extensible_tag(body, bits)?;
    }
    let encoding = match tag {
        TAG_PCM => Encoding::Pcm,
        TAG_FLOAT => Encoding::Float,
        _ => return Err(DecodeError::Unsupported),
    };
    let channels = le_u16(body, 2)?;
    if channels == 0 {
        return Err(DecodeError::Invalid);
    }
    let sample_rate = le_u32(body, 4)?;
    if sample_rate == 0 {
        return Err(DecodeError::Invalid);
    }
    let channels = u8::try_from(channels)
        .ok()
        .filter(|c| *c <= MAX_SOUND_CHANNELS)
        .ok_or(DecodeError::Unsupported)?;
    let bytes_per_sample = bytes_per_sample(encoding, bits)?;
    if usize::from(le_u16(body, 12)?) != bytes_per_sample * usize::from(channels) {
        return Err(DecodeError::Invalid);
    }
    Ok(Format {
        encoding,
        channels,
        sample_rate,
        bytes_per_sample,
    })
}

/// Checks the RIFF header and returns the bytes after it.
fn riff_body(bytes: &[u8]) -> Result<&[u8], DecodeError> {
    let magic = slice(bytes, 0, 4)?;
    if magic != b"RIFF" {
        return Err(DecodeError::NotThisFormat);
    }
    if slice(bytes, 0, RIFF_HEADER_LEN)?.get(8..) != Some(b"WAVE".as_slice()) {
        return Err(DecodeError::NotThisFormat);
    }
    bytes.get(RIFF_HEADER_LEN..).ok_or(DecodeError::Truncated)
}

/// One chunk: its id, its body (bounded by the input) and the offset of the next chunk.
struct Chunk<'a> {
    id: [u8; 4],
    body: &'a [u8],
    next: usize,
}

fn read_chunk(bytes: &[u8], at: usize) -> Result<Chunk<'_>, DecodeError> {
    let id = match slice(bytes, at, 4)? {
        &[a, b, c, d] => [a, b, c, d],
        _ => return Err(DecodeError::Truncated),
    };
    let size = usize::try_from(le_u32(bytes, at + 4)?).map_err(|_| DecodeError::Truncated)?;
    let start = at + CHUNK_HEADER_LEN;
    let body = slice(bytes, start, size)?;
    let next = start
        .checked_add(size)
        .and_then(|end| end.checked_add(size & 1))
        .ok_or(DecodeError::Truncated)?;
    Ok(Chunk { id, body, next })
}

/// Finds the format and the sample bytes.
fn find_format_and_data(bytes: &[u8]) -> Result<(Format, &[u8]), DecodeError> {
    let mut format: Option<Format> = None;
    let mut at = 0;
    while at < bytes.len() {
        let chunk = read_chunk(bytes, at)?;
        match &chunk.id {
            b"fmt " if format.is_some() => return Err(DecodeError::Invalid),
            b"fmt " => format = Some(parse_fmt(chunk.body)?),
            b"data" => {
                let found = format.ok_or(DecodeError::Invalid)?;
                return Ok((found, chunk.body));
            }
            _ => {}
        }
        at = chunk.next;
    }
    Err(if format.is_some() {
        DecodeError::Truncated
    } else {
        DecodeError::Invalid
    })
}

/// Scales an integer sample to `f32`. The division is exact in `f64`; the final rounding to
/// `f32` is the intended one (the largest 32-bit value rounds to exactly 1.0).
#[expect(
    clippy::cast_possible_truncation,
    reason = "the scaled value is within [-1, 1], so only the rounding to f32 is lost"
)]
fn scaled(value: i32, scale: f64) -> f32 {
    (f64::from(value) / scale) as f32
}

fn sample(encoding: Encoding, raw: &[u8]) -> Result<f32, DecodeError> {
    match (encoding, raw) {
        (Encoding::Pcm, &[a]) => Ok((f32::from(a) - SCALE_8) / SCALE_8),
        (Encoding::Pcm, &[a, b]) => Ok(f32::from(i16::from_le_bytes([a, b])) / SCALE_16),
        (Encoding::Pcm, &[a, b, c]) => Ok(scaled(i32::from_le_bytes([0, a, b, c]) >> 8, SCALE_24)),
        (Encoding::Pcm, &[a, b, c, d]) => Ok(scaled(i32::from_le_bytes([a, b, c, d]), SCALE_32)),
        (Encoding::Float, &[a, b, c, d]) => {
            let value = f32::from_le_bytes([a, b, c, d]);
            if value.is_finite() && value.abs() <= MAX_FLOAT_MAGNITUDE {
                Ok(value)
            } else {
                Err(DecodeError::Invalid)
            }
        }
        _ => Err(DecodeError::Invalid),
    }
}

impl IAudioDecoder for WavDecoder {
    fn decode(&self, bytes: &[u8]) -> Result<Sound, DecodeError> {
        let (format, data) = find_format_and_data(riff_body(bytes)?)?;
        let frame_bytes = format.bytes_per_sample * usize::from(format.channels);
        if !data.len().is_multiple_of(frame_bytes) {
            return Err(DecodeError::Invalid);
        }
        let count = data.len() / format.bytes_per_sample;
        if count > self.max_samples {
            return Err(DecodeError::TooLarge);
        }
        let mut samples = Vec::new();
        samples
            .try_reserve_exact(count)
            .map_err(|_| DecodeError::TooLarge)?;
        for raw in data.chunks_exact(format.bytes_per_sample) {
            samples.push(sample(format.encoding, raw)?);
        }
        Sound::new(format.sample_rate, format.channels, samples).map_err(|_| DecodeError::Invalid)
    }
}
