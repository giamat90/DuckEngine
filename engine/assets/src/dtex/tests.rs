use super::*;

fn must<T, E>(r: Result<T, E>) -> T {
    r.unwrap_or_else(|_| unreachable!())
}

fn spec(width: u32, height: u32) -> TextureSpec {
    TextureSpec {
        width,
        height,
        srgb: false,
    }
}

/// A valid one-mip file of `width` by `height` filled with `fill`.
fn file(width: u32, height: u32, fill: u8) -> Vec<u8> {
    let pixels = vec![fill; usize::try_from(width * height * 4).unwrap_or_else(|_| unreachable!())];
    let len = must(encoded_len(spec(width, height), 1));
    let mut out = vec![0u8; len];
    must(encode_into(&mut out, spec(width, height), &[&pixels]));
    out
}

#[test]
fn round_trip_one_mip() {
    let bytes = file(2, 3, 7);
    let tex = must(Texture::parse(&bytes));
    assert_eq!((tex.width(), tex.height(), tex.mip_count()), (2, 3, 1));
    assert!(!tex.is_srgb());
    let mip = tex.mip(0).unwrap_or_else(|| unreachable!());
    assert_eq!((mip.width, mip.height, mip.bytes.len()), (2, 3, 24));
    assert!(mip.bytes.iter().all(|&b| b == 7));
}

#[test]
fn header_layout_is_fixed() {
    let bytes = file(5, 9, 0);
    assert_eq!(&bytes[0..4], b"DTEX");
    assert_eq!(bytes[4..6], 0u16.to_le_bytes());
    assert_eq!(bytes[6], 0);
    assert_eq!(bytes[7], 0);
    assert_eq!(bytes[8..12], 5u32.to_le_bytes());
    assert_eq!(bytes[12..16], 9u32.to_le_bytes());
    assert_eq!(bytes[16..20], 1u32.to_le_bytes());
    assert_eq!(bytes.len(), HEADER_LEN + 5 * 9 * 4);
    assert_eq!(HEADER_LEN, 20);
}

#[test]
fn srgb_flag_round_trips() {
    let pixels = [0u8; 4];
    let s = TextureSpec {
        width: 1,
        height: 1,
        srgb: true,
    };
    let mut out = vec![0u8; must(encoded_len(s, 1))];
    must(encode_into(&mut out, s, &[&pixels]));
    assert_eq!(out[7], 1);
    assert!(must(Texture::parse(&out)).is_srgb());
}

#[test]
fn mip_chain_sizes_and_order() {
    let s = spec(4, 2);
    assert_eq!(must(encoded_len(s, 3)), HEADER_LEN + 32 + 8 + 4);
    let (m0, m1, m2) = ([1u8; 32], [2u8; 8], [3u8; 4]);
    let mut out = vec![0u8; must(encoded_len(s, 3))];
    must(encode_into(&mut out, s, &[&m0, &m1, &m2]));
    let tex = must(Texture::parse(&out));
    assert_eq!(tex.mip_count(), 3);
    let dims: Vec<(u32, u32, u8)> = (0..3)
        .map(|i| {
            let m = tex.mip(i).unwrap_or_else(|| unreachable!());
            (m.width, m.height, m.bytes[0])
        })
        .collect();
    assert_eq!(dims, vec![(4, 2, 1), (2, 1, 2), (1, 1, 3)]);
    assert!(tex.mip(3).is_none());
}

#[test]
fn mip_views_on_a_non_square_odd_chain() {
    // 5x3 -> 2x1 -> 1x1: byte lengths 60, 8, 4 with distinct fill values.
    let s = spec(5, 3);
    let (m0, m1, m2) = ([10u8; 60], [20u8; 8], [30u8; 4]);
    let mut out = vec![0u8; must(encoded_len(s, 3))];
    must(encode_into(&mut out, s, &[&m0, &m1, &m2]));
    let tex = must(Texture::parse(&out));
    let view = |i| tex.mip(i).unwrap_or_else(|| unreachable!());
    assert_eq!(
        (view(0).width, view(0).height, view(0).bytes),
        (5, 3, &m0[..])
    );
    assert_eq!(
        (view(1).width, view(1).height, view(1).bytes),
        (2, 1, &m1[..])
    );
    assert_eq!(
        (view(2).width, view(2).height, view(2).bytes),
        (1, 1, &m2[..])
    );
    assert!(tex.mip(3).is_none());
}

#[test]
fn mip_size_never_below_one() {
    assert_eq!(mip_dimensions(1, 1, 0), (1, 1));
    assert_eq!(mip_dimensions(8, 1, 2), (2, 1));
    assert_eq!(mip_dimensions(5, 3, 1), (2, 1));
}

#[test]
fn max_mip_count_is_the_full_chain() {
    assert_eq!(max_mip_count(1, 1), 1);
    assert_eq!(max_mip_count(4, 2), 3);
    assert_eq!(max_mip_count(5, 3), 3);
    assert_eq!(max_mip_count(MAX_DIMENSION, 1), 15);
}

#[test]
fn rejects_empty_input_and_short_header() {
    assert_eq!(Texture::parse(&[]).err(), Some(DtexError::Truncated));
    let bytes = file(1, 1, 0);
    assert_eq!(
        Texture::parse(&bytes[..HEADER_LEN - 1]).err(),
        Some(DtexError::Truncated)
    );
}

#[test]
fn rejects_wrong_magic() {
    let mut bytes = file(1, 1, 0);
    bytes[0] = b'X';
    assert_eq!(Texture::parse(&bytes).err(), Some(DtexError::BadMagic));
}

#[test]
fn rejects_unknown_version_and_format_and_flags() {
    let mut a = file(1, 1, 0);
    a[4] = 1;
    assert_eq!(
        Texture::parse(&a).err(),
        Some(DtexError::UnsupportedVersion(1))
    );
    let mut b = file(1, 1, 0);
    b[6] = 9;
    assert_eq!(
        Texture::parse(&b).err(),
        Some(DtexError::UnsupportedFormat(9))
    );
    let mut c = file(1, 1, 0);
    c[7] = 0b10;
    assert_eq!(Texture::parse(&c).err(), Some(DtexError::ReservedFlags));
}

#[test]
fn rejects_zero_and_huge_dimensions() {
    for (w, h) in [
        (0u32, 1u32),
        (1, 0),
        (MAX_DIMENSION + 1, 1),
        (1, MAX_DIMENSION + 1),
        (u32::MAX, u32::MAX),
        // Each upper byte of the two fields must be read: these are only huge through that byte.
        (0x0001_0001, 1),
        (1, 0x0001_0001),
        (0x0100_0001, 1),
        (1, 0x0100_0001),
    ] {
        let mut bytes = file(1, 1, 0);
        bytes[8..12].copy_from_slice(&w.to_le_bytes());
        bytes[12..16].copy_from_slice(&h.to_le_bytes());
        assert_eq!(
            Texture::parse(&bytes).err(),
            Some(DtexError::BadDimensions),
            "{w}x{h}"
        );
    }
}

#[test]
fn accepts_the_maximum_dimension_header_only_when_data_is_present() {
    let mut bytes = file(1, 1, 0);
    bytes[8..12].copy_from_slice(&MAX_DIMENSION.to_le_bytes());
    bytes[12..16].copy_from_slice(&MAX_DIMENSION.to_le_bytes());
    assert_eq!(Texture::parse(&bytes).err(), Some(DtexError::Truncated));
}

#[test]
fn rejects_bad_mip_counts() {
    for count in [0u32, 2, u32::MAX] {
        let mut bytes = file(1, 1, 0);
        bytes[16..20].copy_from_slice(&count.to_le_bytes());
        assert_eq!(
            Texture::parse(&bytes).err(),
            Some(DtexError::BadMipCount),
            "{count}"
        );
    }
}

#[test]
fn rejects_truncated_and_trailing_data() {
    let bytes = file(2, 2, 1);
    assert_eq!(
        Texture::parse(&bytes[..bytes.len() - 1]).err(),
        Some(DtexError::Truncated)
    );
    let mut longer = bytes.clone();
    longer.push(0);
    assert_eq!(Texture::parse(&longer).err(), Some(DtexError::TrailingData));
}

#[test]
fn hostile_input_never_panics() {
    let base = file(3, 3, 5);
    for i in 0..base.len() {
        for value in [0u8, 1, 0x7F, 0xFF] {
            let mut bytes = base.clone();
            bytes[i] = value;
            let _ = Texture::parse(&bytes);
            let _ = Texture::parse(&bytes[..i]);
        }
    }
}

#[test]
fn encode_rejects_wrong_arguments() {
    let pixels = [0u8; 4];
    let mut out = vec![0u8; 64];
    assert_eq!(
        encode_into(&mut out, spec(0, 1), &[&pixels]),
        Err(DtexError::BadDimensions)
    );
    assert_eq!(
        encode_into(&mut out, spec(1, 1), &[]),
        Err(DtexError::BadMipCount)
    );
    assert_eq!(
        encode_into(&mut out, spec(1, 1), &[&pixels, &pixels]),
        Err(DtexError::BadMipCount)
    );
    assert_eq!(
        encode_into(&mut out, spec(2, 1), &[&pixels]),
        Err(DtexError::BadMipSize)
    );
    let mut small = vec![0u8; HEADER_LEN + 3];
    assert_eq!(
        encode_into(&mut small, spec(1, 1), &[&pixels]),
        Err(DtexError::Truncated)
    );
}

#[test]
fn encoded_len_rejects_bad_arguments() {
    assert_eq!(encoded_len(spec(0, 4), 1), Err(DtexError::BadDimensions));
    assert_eq!(encoded_len(spec(4, 4), 0), Err(DtexError::BadMipCount));
    assert_eq!(encoded_len(spec(4, 4), 4), Err(DtexError::BadMipCount));
    assert_eq!(must(encoded_len(spec(4, 4), 3)), HEADER_LEN + 64 + 16 + 4);
}

#[test]
fn encode_returns_bytes_written() {
    let pixels = [9u8; 16];
    let mut out = vec![0u8; 100];
    assert_eq!(
        must(encode_into(&mut out, spec(2, 2), &[&pixels])),
        HEADER_LEN + 16
    );
}

#[test]
fn every_error_has_its_own_message() {
    let all = [
        DtexError::Truncated,
        DtexError::BadMagic,
        DtexError::UnsupportedVersion(1),
        DtexError::UnsupportedFormat(1),
        DtexError::ReservedFlags,
        DtexError::BadDimensions,
        DtexError::BadMipCount,
        DtexError::BadMipSize,
        DtexError::TrailingData,
    ];
    let texts: Vec<String> = all.iter().map(ToString::to_string).collect();
    for (i, a) in texts.iter().enumerate() {
        assert!(!a.is_empty());
        assert!(texts[i + 1..].iter().all(|b| a != b));
    }
}
