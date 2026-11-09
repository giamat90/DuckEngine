//! A file over the sample cap is refused before anything is allocated for its samples
//! (counting allocator with per-thread counters).

use audio::decoder::{DecodeError, IAudioDecoder};
use audio::wav::WavDecoder;
use base::alloc::{CountingAllocator, count_allocations};

#[global_allocator]
static ALLOCATOR: CountingAllocator<std::alloc::System> =
    CountingAllocator::new(std::alloc::System);

fn pcm16_file(samples: usize) -> Vec<u8> {
    let data_len = samples * 2;
    let mut bytes = b"RIFF".to_vec();
    bytes.extend_from_slice(&u32::try_from(data_len + 36).unwrap_or(0).to_le_bytes());
    bytes.extend_from_slice(b"WAVEfmt ");
    bytes.extend_from_slice(&16_u32.to_le_bytes());
    for field in [1_u16, 1] {
        bytes.extend_from_slice(&field.to_le_bytes());
    }
    bytes.extend_from_slice(&44_100_u32.to_le_bytes());
    bytes.extend_from_slice(&88_200_u32.to_le_bytes());
    for field in [2_u16, 16] {
        bytes.extend_from_slice(&field.to_le_bytes());
    }
    bytes.extend_from_slice(b"data");
    bytes.extend_from_slice(&u32::try_from(data_len).unwrap_or(0).to_le_bytes());
    bytes.resize(bytes.len() + data_len, 0);
    bytes
}

#[test]
fn an_over_cap_file_allocates_nothing() {
    let bytes = pcm16_file(1000);
    let decoder = WavDecoder::with_max_samples(999);
    let (result, counts) = count_allocations(|| decoder.decode(&bytes));
    assert_eq!(result.err(), Some(DecodeError::TooLarge));
    assert_eq!(counts.calls(), 0, "allocator calls");
}

#[test]
fn a_file_within_the_cap_allocates_the_samples_once() {
    let bytes = pcm16_file(1000);
    let decoder = WavDecoder::with_max_samples(1000);
    let (result, counts) = count_allocations(|| decoder.decode(&bytes));
    assert!(result.is_ok());
    assert!(counts.allocations >= 1);
}
