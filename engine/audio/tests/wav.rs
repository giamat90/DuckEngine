//! Tests of the WAV decoder (card 5-4). Files are built from bytes in the tests; the input of a
//! decoder is untrusted, so most tests are about files that are wrong.

use audio::decoder::{DecodeError, IAudioDecoder};
use audio::device::AudioRenderer;
use audio::format::{ChannelLayout, StreamFormat};
use audio::mixer::{Mixer, PlayParams};
use audio::wav::WavDecoder;
use std::error::Error;
use std::sync::Arc;

type TestResult = Result<(), Box<dyn Error>>;

const PCM: u16 = 1;
const FLOAT: u16 = 3;
const EXTENSIBLE: u16 = 0xFFFE;
/// The tail of the standard sub-format GUID, after the two bytes of the format tag.
const GUID_TAIL: [u8; 14] = [
    0x00, 0x00, 0x00, 0x00, 0x10, 0x00, 0x80, 0x00, 0x00, 0xAA, 0x00, 0x38, 0x9B, 0x71,
];

fn chunk(id: &str, body: &[u8]) -> Vec<u8> {
    let mut bytes = id.as_bytes().to_vec();
    bytes.extend_from_slice(&u32::try_from(body.len()).unwrap_or(u32::MAX).to_le_bytes());
    bytes.extend_from_slice(body);
    if !body.len().is_multiple_of(2) {
        bytes.push(0);
    }
    bytes
}

fn riff(chunks: &[Vec<u8>]) -> Vec<u8> {
    let body: Vec<u8> = chunks.iter().flatten().copied().collect();
    let mut bytes = b"RIFF".to_vec();
    bytes.extend_from_slice(&u32::try_from(body.len() + 4).unwrap_or(0).to_le_bytes());
    bytes.extend_from_slice(b"WAVE");
    bytes.extend_from_slice(&body);
    bytes
}

/// The 16-byte core of a `fmt ` chunk with a consistent block align.
fn fmt_body(tag: u16, channels: u16, rate: u32, bits: u16) -> Vec<u8> {
    let align = channels * (bits / 8);
    let mut body = tag.to_le_bytes().to_vec();
    body.extend_from_slice(&channels.to_le_bytes());
    body.extend_from_slice(&rate.to_le_bytes());
    body.extend_from_slice(&(rate * u32::from(align)).to_le_bytes());
    body.extend_from_slice(&align.to_le_bytes());
    body.extend_from_slice(&bits.to_le_bytes());
    body
}

fn wav(tag: u16, channels: u16, rate: u32, bits: u16, data: &[u8]) -> Vec<u8> {
    riff(&[
        chunk("fmt ", &fmt_body(tag, channels, rate, bits)),
        chunk("data", data),
    ])
}

fn pcm16(samples: &[i16]) -> Vec<u8> {
    let data: Vec<u8> = samples.iter().flat_map(|s| s.to_le_bytes()).collect();
    wav(PCM, 1, 44_100, 16, &data)
}

/// An extensible `fmt ` chunk (40 bytes).
fn extensible_body(sub_tag: u16, channels: u16, bits: u16, valid_bits: u16) -> Vec<u8> {
    let mut body = fmt_body(EXTENSIBLE, channels, 48_000, bits);
    body.extend_from_slice(&22_u16.to_le_bytes());
    body.extend_from_slice(&valid_bits.to_le_bytes());
    body.extend_from_slice(&0_u32.to_le_bytes());
    body.extend_from_slice(&sub_tag.to_le_bytes());
    body.extend_from_slice(&GUID_TAIL);
    body
}

fn decode(bytes: &[u8]) -> Result<audio::sound::Sound, DecodeError> {
    WavDecoder::new().decode(bytes)
}

fn samples_of(bytes: &[u8]) -> Result<Vec<f32>, Box<dyn Error>> {
    Ok(decode(bytes)?.samples().to_vec())
}

fn assert_samples(actual: &[f32], expected: &[f32]) {
    assert_eq!(actual.len(), expected.len());
    for (i, (a, e)) in actual.iter().zip(expected).enumerate() {
        assert!((a - e).abs() < 1.0e-7, "sample {i}: {a} differs from {e}");
    }
}

#[test]
fn sixteen_bit_pcm_decodes_to_the_expected_samples() -> TestResult {
    let sound = decode(&pcm16(&[0, 16_384, -16_384, -32_768, 32_767]))?;
    assert_eq!((sound.sample_rate(), sound.channels()), (44_100, 1));
    assert_samples(
        sound.samples(),
        &[0.0, 0.5, -0.5, -1.0, 32_767.0 / 32_768.0],
    );
    Ok(())
}

#[test]
fn eight_bit_pcm_is_unsigned_around_128() -> TestResult {
    let samples = samples_of(&wav(PCM, 1, 8_000, 8, &[0, 128, 255, 192]))?;
    assert_samples(&samples, &[-1.0, 0.0, 127.0 / 128.0, 0.5]);
    Ok(())
}

#[test]
fn twenty_four_bit_pcm_is_signed_and_sign_extended() -> TestResult {
    let data = [0x00, 0x00, 0x40, 0x00, 0x00, 0x80, 0xFF, 0xFF, 0xFF];
    let samples = samples_of(&wav(PCM, 1, 48_000, 24, &data))?;
    assert_samples(&samples, &[0.5, -1.0, -1.0 / 8_388_608.0]);
    Ok(())
}

#[test]
fn thirty_two_bit_pcm_is_signed() -> TestResult {
    let data: Vec<u8> = [0x4000_0000_i32, i32::MIN, 0]
        .iter()
        .flat_map(|s| s.to_le_bytes())
        .collect();
    let samples = samples_of(&wav(PCM, 1, 48_000, 32, &data))?;
    assert_samples(&samples, &[0.5, -1.0, 0.0]);
    Ok(())
}

#[test]
fn float_samples_pass_through_exactly() -> TestResult {
    let values = [0.25_f32, -0.75, 1.5, 0.0];
    let data: Vec<u8> = values.iter().flat_map(|s| s.to_le_bytes()).collect();
    let sound = decode(&wav(FLOAT, 1, 22_050, 32, &data))?;
    assert_eq!(sound.sample_rate(), 22_050);
    assert_eq!(sound.samples(), values);
    Ok(())
}

#[test]
fn channels_stay_interleaved() -> TestResult {
    let data: Vec<u8> = [100_i16, -100, 200, -200]
        .iter()
        .flat_map(|s| s.to_le_bytes())
        .collect();
    let sound = decode(&wav(PCM, 2, 44_100, 16, &data))?;
    assert_eq!((sound.channels(), sound.frame_count()), (2, 2));
    assert_samples(
        sound.samples(),
        &[
            100.0 / 32_768.0,
            -100.0 / 32_768.0,
            200.0 / 32_768.0,
            -200.0 / 32_768.0,
        ],
    );
    Ok(())
}

#[test]
fn eight_channels_are_accepted() -> TestResult {
    let sound = decode(&wav(PCM, 8, 48_000, 16, &[0; 32]))?;
    assert_eq!((sound.channels(), sound.frame_count()), (8, 2));
    Ok(())
}

#[test]
fn the_extensible_form_decodes_pcm_and_float() -> TestResult {
    let pcm = riff(&[
        chunk("fmt ", &extensible_body(PCM, 1, 16, 16)),
        chunk("data", &16_384_i16.to_le_bytes()),
    ]);
    assert_samples(&samples_of(&pcm)?, &[0.5]);
    let float = riff(&[
        chunk("fmt ", &extensible_body(FLOAT, 1, 32, 32)),
        chunk("data", &0.125_f32.to_le_bytes()),
    ]);
    assert_samples(&samples_of(&float)?, &[0.125]);
    let packed = riff(&[
        chunk("fmt ", &extensible_body(PCM, 1, 32, 24)),
        chunk("data", &0x4000_0000_i32.to_le_bytes()),
    ]);
    assert_samples(&samples_of(&packed)?, &[0.5]);
    Ok(())
}

#[test]
fn a_bad_extensible_chunk_is_refused() {
    let short = riff(&[
        chunk("fmt ", &fmt_body(EXTENSIBLE, 1, 48_000, 16)),
        chunk("data", &[0, 0]),
    ]);
    assert_eq!(decode(&short).err(), Some(DecodeError::Invalid));
    let wide = riff(&[
        chunk("fmt ", &extensible_body(PCM, 1, 16, 24)),
        chunk("data", &[0, 0]),
    ]);
    assert_eq!(decode(&wide).err(), Some(DecodeError::Invalid));
    let mut body = extensible_body(PCM, 1, 16, 16);
    body[30] = 0x55;
    let guid = riff(&[chunk("fmt ", &body), chunk("data", &[0, 0])]);
    assert_eq!(decode(&guid).err(), Some(DecodeError::Unsupported));
    let adpcm = riff(&[
        chunk("fmt ", &extensible_body(2, 1, 16, 16)),
        chunk("data", &[0, 0]),
    ]);
    assert_eq!(decode(&adpcm).err(), Some(DecodeError::Unsupported));
}

#[test]
fn unknown_chunks_are_skipped_including_odd_sized_ones() -> TestResult {
    let bytes = riff(&[
        chunk("LIST", b"abc"),
        chunk("fmt ", &fmt_body(PCM, 1, 44_100, 16)),
        chunk("fact", &[1, 2, 3, 4]),
        chunk("junk", &[9]),
        chunk("data", &16_384_i16.to_le_bytes()),
        chunk("LIST", b"after the data"),
    ]);
    assert_samples(&samples_of(&bytes)?, &[0.5]);
    Ok(())
}

#[test]
fn a_wrong_riff_size_field_is_not_trusted() -> TestResult {
    let mut bytes = pcm16(&[16_384]);
    bytes[4..8].copy_from_slice(&u32::MAX.to_le_bytes());
    assert_samples(&samples_of(&bytes)?, &[0.5]);
    bytes[4..8].copy_from_slice(&0_u32.to_le_bytes());
    assert_samples(&samples_of(&bytes)?, &[0.5]);
    Ok(())
}

#[test]
fn a_file_without_samples_gives_an_empty_sound() -> TestResult {
    let sound = decode(&pcm16(&[]))?;
    assert_eq!(sound.frame_count(), 0);
    Ok(())
}

#[test]
fn bytes_that_are_not_a_wav_file_are_refused_by_name() {
    assert_eq!(decode(&[]).err(), Some(DecodeError::Truncated));
    assert_eq!(decode(b"RIF").err(), Some(DecodeError::Truncated));
    assert_eq!(decode(b"RIFF").err(), Some(DecodeError::Truncated));
    assert_eq!(decode(b"RIFFxxxxWAV").err(), Some(DecodeError::Truncated));
    assert_eq!(
        decode(b"RIFXxxxxWAVE").err(),
        Some(DecodeError::NotThisFormat)
    );
    assert_eq!(
        decode(b"OggS and so on").err(),
        Some(DecodeError::NotThisFormat)
    );
    assert_eq!(
        decode(b"RIFFxxxxAVI LIST").err(),
        Some(DecodeError::NotThisFormat)
    );
}

#[test]
fn every_truncation_of_a_good_file_is_an_error() {
    let good = pcm16(&[1, 2, 3, 4, 5, 6]);
    for length in 0..good.len() {
        let error = decode(&good[..length]).err();
        assert!(
            matches!(error, Some(DecodeError::Truncated | DecodeError::Invalid)),
            "prefix of {length} bytes: {error:?}"
        );
    }
    assert!(decode(&good).is_ok());
}

#[test]
fn a_data_size_larger_than_the_file_is_truncated() {
    let mut bytes = pcm16(&[1, 2, 3, 4]);
    let size_at = bytes.len() - 8 - 4;
    bytes[size_at..size_at + 4].copy_from_slice(&1000_u32.to_le_bytes());
    assert_eq!(decode(&bytes).err(), Some(DecodeError::Truncated));
    bytes[size_at..size_at + 4].copy_from_slice(&u32::MAX.to_le_bytes());
    assert_eq!(decode(&bytes).err(), Some(DecodeError::Truncated));
}

#[test]
fn a_chunk_size_beyond_the_input_is_refused_without_panicking() {
    let bytes = riff(&[
        {
            let mut c = b"junk".to_vec();
            c.extend_from_slice(&u32::MAX.to_le_bytes());
            c
        },
        chunk("fmt ", &fmt_body(PCM, 1, 44_100, 16)),
    ]);
    assert_eq!(decode(&bytes).err(), Some(DecodeError::Truncated));
}

fn fmt_error(body: &[u8]) -> Option<DecodeError> {
    decode(&riff(&[chunk("fmt ", body), chunk("data", &[0; 8])])).err()
}

#[test]
fn a_zero_channel_count_or_sample_rate_is_invalid() {
    assert_eq!(
        fmt_error(&fmt_body(PCM, 0, 44_100, 16)),
        Some(DecodeError::Invalid)
    );
    assert_eq!(
        fmt_error(&fmt_body(PCM, 1, 0, 16)),
        Some(DecodeError::Invalid)
    );
    assert_eq!(
        fmt_error(&fmt_body(PCM, 1, 44_100, 0)),
        Some(DecodeError::Invalid)
    );
}

#[test]
fn more_than_eight_channels_are_unsupported() {
    assert_eq!(
        fmt_error(&fmt_body(PCM, 9, 44_100, 16)),
        Some(DecodeError::Unsupported)
    );
    assert_eq!(
        fmt_error(&fmt_body(PCM, u16::MAX, 44_100, 8)),
        Some(DecodeError::Unsupported)
    );
}

#[test]
fn compressed_formats_and_odd_bit_depths_are_unsupported() {
    assert_eq!(
        fmt_error(&fmt_body(2, 1, 44_100, 4)),
        Some(DecodeError::Unsupported)
    );
    assert_eq!(
        fmt_error(&fmt_body(0x55, 1, 44_100, 16)),
        Some(DecodeError::Unsupported)
    );
    assert_eq!(
        fmt_error(&fmt_body(PCM, 1, 44_100, 12)),
        Some(DecodeError::Unsupported)
    );
    assert_eq!(
        fmt_error(&fmt_body(FLOAT, 1, 44_100, 64)),
        Some(DecodeError::Unsupported)
    );
    assert_eq!(
        fmt_error(&fmt_body(FLOAT, 1, 44_100, 16)),
        Some(DecodeError::Unsupported)
    );
}

#[test]
fn an_inconsistent_block_align_or_a_partial_frame_is_invalid() {
    let mut body = fmt_body(PCM, 2, 44_100, 16);
    body[12..14].copy_from_slice(&3_u16.to_le_bytes());
    assert_eq!(fmt_error(&body), Some(DecodeError::Invalid));
    let partial = wav(PCM, 2, 44_100, 16, &[0; 6]);
    assert_eq!(decode(&partial).err(), Some(DecodeError::Invalid));
}

#[test]
fn a_misplaced_or_missing_chunk_is_refused() {
    let short_fmt = riff(&[
        chunk("fmt ", &fmt_body(PCM, 1, 44_100, 16)[..14]),
        chunk("data", &[0; 2]),
    ]);
    assert_eq!(decode(&short_fmt).err(), Some(DecodeError::Invalid));
    let twice = riff(&[
        chunk("fmt ", &fmt_body(PCM, 1, 44_100, 16)),
        chunk("fmt ", &fmt_body(PCM, 1, 44_100, 16)),
        chunk("data", &[0; 2]),
    ]);
    assert_eq!(decode(&twice).err(), Some(DecodeError::Invalid));
    let data_first = riff(&[
        chunk("data", &[0; 2]),
        chunk("fmt ", &fmt_body(PCM, 1, 44_100, 16)),
    ]);
    assert_eq!(decode(&data_first).err(), Some(DecodeError::Invalid));
    let no_data = riff(&[chunk("fmt ", &fmt_body(PCM, 1, 44_100, 16))]);
    assert_eq!(decode(&no_data).err(), Some(DecodeError::Truncated));
    let no_fmt = riff(&[chunk("LIST", b"abcd")]);
    assert_eq!(decode(&no_fmt).err(), Some(DecodeError::Invalid));
}

#[test]
fn a_float_file_with_a_non_finite_sample_is_invalid() {
    for value in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY] {
        let bytes = wav(FLOAT, 1, 44_100, 32, &value.to_le_bytes());
        assert_eq!(decode(&bytes).err(), Some(DecodeError::Invalid));
    }
}

#[test]
fn the_default_cap_is_64_mebi_samples() {
    assert_eq!(audio::wav::DEFAULT_MAX_SAMPLES, 67_108_864);
}

#[test]
fn the_decoded_size_is_capped_before_anything_is_allocated() -> TestResult {
    let bytes = pcm16(&[1, 2, 3, 4, 5]);
    assert_eq!(
        WavDecoder::with_max_samples(4).decode(&bytes).err(),
        Some(DecodeError::TooLarge)
    );
    assert_eq!(
        WavDecoder::with_max_samples(5)
            .decode(&bytes)?
            .frame_count(),
        5
    );
    assert_eq!(
        WavDecoder::with_max_samples(0)
            .decode(&pcm16(&[]))?
            .frame_count(),
        0
    );
    Ok(())
}

/// A small deterministic generator (xorshift), so the mutation test is repeatable.
struct Xorshift(u64);

impl Xorshift {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }

    fn below(&mut self, bound: usize) -> usize {
        usize::try_from(self.next() % u64::try_from(bound).unwrap_or(1)).unwrap_or(0)
    }
}

fn seed_files() -> Vec<Vec<u8>> {
    vec![
        pcm16(&[1, -2, 3, -4, 5, -6]),
        wav(PCM, 2, 8_000, 8, &[1, 2, 3, 4, 5, 6]),
        wav(FLOAT, 1, 48_000, 32, &[0, 0, 0x80, 0x3F, 0, 0, 0, 0]),
        riff(&[
            chunk("LIST", b"info"),
            chunk("fmt ", &extensible_body(PCM, 2, 24, 24)),
            chunk("data", &[0; 12]),
        ]),
    ]
}

/// Every byte of `file` set to a few extreme values, one at a time.
fn single_byte_mutations(decoder: WavDecoder, file: &[u8]) {
    for at in 0..file.len() {
        for value in [0_u8, 1, 0x7F, 0x80, 0xFF] {
            let mut bytes = file.to_vec();
            bytes[at] = value;
            let _ = decoder.decode(&bytes);
        }
    }
}

/// A few random bytes changed and the file cut short.
fn random_mutation(decoder: WavDecoder, file: &[u8], rng: &mut Xorshift) {
    let mut bytes = file.to_vec();
    for _ in 0..=rng.below(4) {
        let at = rng.below(bytes.len());
        bytes[at] = u8::try_from(rng.next() & 0xFF).unwrap_or(0);
    }
    bytes.truncate(1 + rng.below(bytes.len()));
    if let Ok(sound) = decoder.decode(&bytes) {
        assert!(sound.samples().iter().all(|s| s.is_finite()));
    }
}

#[test]
fn mutated_headers_give_errors_or_sounds_but_never_panic() {
    let decoder = WavDecoder::with_max_samples(1 << 16);
    let mut rng = Xorshift(0x9E37_79B9_7F4A_7C15);
    for file in seed_files() {
        single_byte_mutations(decoder, &file);
        for _ in 0..3000 {
            random_mutation(decoder, &file, &mut rng);
        }
    }
}

#[test]
fn a_decoded_sound_plays_in_the_mixer() -> TestResult {
    let decoder: Arc<dyn IAudioDecoder> = Arc::new(WavDecoder::new());
    let sound = Arc::new(decoder.decode(&wav(PCM, 1, 48_000, 16, &[0x00, 0x40, 0x00, 0x20]))?);
    let format = StreamFormat {
        sample_rate: 48_000,
        layout: ChannelLayout::Stereo,
        max_block_frames: 64,
    };
    let mut mixer = Mixer::new(format, 2)?;
    mixer.play(
        sound,
        PlayParams {
            pan: -1.0,
            ..PlayParams::default()
        },
    )?;
    let mut out = [0.0_f32; 6];
    mixer.render(&mut out, 3);
    assert_samples(&[out[0], out[2], out[4]], &[0.5, 0.25, 0.0]);
    Ok(())
}

#[test]
fn a_final_data_chunk_of_odd_length_needs_no_pad_byte() -> TestResult {
    let mut bytes = wav(PCM, 1, 8_000, 8, &[128, 192, 64]);
    assert_eq!(bytes.pop(), Some(0));
    assert_samples(&samples_of(&bytes)?, &[0.0, 0.5, -0.5]);
    Ok(())
}

#[test]
fn a_fmt_chunk_of_eighteen_bytes_is_read_like_the_short_one() -> TestResult {
    let mut body = fmt_body(PCM, 1, 44_100, 16);
    body.extend_from_slice(&0_u16.to_le_bytes());
    let bytes = riff(&[
        chunk("fmt ", &body),
        chunk("data", &16_384_i16.to_le_bytes()),
    ]);
    assert_samples(&samples_of(&bytes)?, &[0.5]);
    Ok(())
}

#[test]
fn the_extreme_integer_samples_decode_to_the_ends_of_the_range() -> TestResult {
    let max32 = wav(PCM, 1, 48_000, 32, &i32::MAX.to_le_bytes());
    assert_samples(&samples_of(&max32)?, &[1.0]);
    let max24 = wav(PCM, 1, 48_000, 24, &[0xFF, 0xFF, 0x7F]);
    assert_samples(&samples_of(&max24)?, &[8_388_607.0 / 8_388_608.0]);
    Ok(())
}

#[test]
fn a_block_align_that_does_not_match_is_invalid_at_every_depth() {
    for (tag, bits) in [(PCM, 8_u16), (PCM, 24), (PCM, 32), (FLOAT, 32)] {
        let mut body = fmt_body(tag, 1, 44_100, bits);
        let wrong = bits / 8 + 1;
        body[12..14].copy_from_slice(&wrong.to_le_bytes());
        assert_eq!(fmt_error(&body), Some(DecodeError::Invalid), "{bits} bits");
    }
}

#[test]
fn the_first_data_chunk_wins_and_a_huge_sample_rate_is_kept() -> TestResult {
    let bytes = riff(&[
        chunk("fmt ", &fmt_body(PCM, 1, u32::MAX / 2, 16)),
        chunk("data", &16_384_i16.to_le_bytes()),
        chunk("data", &(-16_384_i16).to_le_bytes()),
    ]);
    let sound = decode(&bytes)?;
    assert_eq!(sound.sample_rate(), u32::MAX / 2);
    assert_samples(sound.samples(), &[0.5]);
    Ok(())
}

#[test]
fn extensible_valid_bits_may_be_zero_up_to_the_container_size() -> TestResult {
    for valid in [0_u16, 8, 16] {
        let bytes = riff(&[
            chunk("fmt ", &extensible_body(PCM, 1, 16, valid)),
            chunk("data", &16_384_i16.to_le_bytes()),
        ]);
        assert_samples(&samples_of(&bytes)?, &[0.5]);
    }
    let eight = riff(&[
        chunk("fmt ", &extensible_body(PCM, 1, 8, 8)),
        chunk("data", &[192]),
    ]);
    assert_samples(&samples_of(&eight)?, &[0.5]);
    Ok(())
}

#[test]
fn float_samples_beyond_a_sane_headroom_are_invalid() -> TestResult {
    let loud = wav(FLOAT, 1, 44_100, 32, &1.0e6_f32.to_le_bytes());
    assert_samples(&samples_of(&loud)?, &[1.0e6]);
    let too_loud = wav(FLOAT, 1, 44_100, 32, &3.0e38_f32.to_le_bytes());
    assert_eq!(decode(&too_loud).err(), Some(DecodeError::Invalid));
    let just_over = wav(FLOAT, 1, 44_100, 32, &1.000_001e6_f32.to_le_bytes());
    assert_eq!(decode(&just_over).err(), Some(DecodeError::Invalid));
    Ok(())
}
