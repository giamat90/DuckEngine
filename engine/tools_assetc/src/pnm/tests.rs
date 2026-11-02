use super::*;

fn must<T, E>(r: Result<T, E>) -> T {
    r.unwrap_or_else(|_| unreachable!())
}

#[test]
fn p6_rgb_gets_opaque_alpha() {
    let mut bytes = b"P6\n2 1\n255\n".to_vec();
    bytes.extend_from_slice(&[1, 2, 3, 4, 5, 6]);
    let img = must(parse(&bytes));
    assert_eq!((img.width, img.height), (2, 1));
    assert_eq!(img.rgba, vec![1, 2, 3, 255, 4, 5, 6, 255]);
}

#[test]
fn p6_comments_and_odd_whitespace() {
    let mut bytes = b"P6 # a comment\n# another\n1\t1\r\n255 ".to_vec();
    bytes.extend_from_slice(&[9, 8, 7]);
    assert_eq!(must(parse(&bytes)).rgba, vec![9, 8, 7, 255]);
}

#[test]
fn p7_rgb_alpha_keeps_alpha() {
    let mut bytes =
        b"P7\nWIDTH 1\nHEIGHT 2\nDEPTH 4\nMAXVAL 255\nTUPLTYPE RGB_ALPHA\nENDHDR\n".to_vec();
    bytes.extend_from_slice(&[1, 2, 3, 4, 5, 6, 7, 8]);
    let img = must(parse(&bytes));
    assert_eq!((img.width, img.height), (1, 2));
    assert_eq!(img.rgba, vec![1, 2, 3, 4, 5, 6, 7, 8]);
}

#[test]
fn p7_rgb_depth_three() {
    let mut bytes = b"P7\nWIDTH 1\nHEIGHT 1\nDEPTH 3\nMAXVAL 255\nTUPLTYPE RGB\nENDHDR\n".to_vec();
    bytes.extend_from_slice(&[1, 2, 3]);
    assert_eq!(must(parse(&bytes)).rgba, vec![1, 2, 3, 255]);
}

#[test]
fn p7_missing_fields_are_a_bad_header() {
    let bytes = b"P7\nWIDTH 1\nDEPTH 3\nMAXVAL 255\nENDHDR\n\x01\x02\x03".to_vec();
    assert_eq!(parse(&bytes), Err(ConvertError::BadHeader));
}

#[test]
fn unknown_magic() {
    assert_eq!(
        parse(b"P5\n1 1\n255\n\x00"),
        Err(ConvertError::UnknownFormat)
    );
    assert_eq!(parse(b""), Err(ConvertError::UnknownFormat));
    assert_eq!(parse(b"\x89PNG\r\n"), Err(ConvertError::UnknownFormat));
}

#[test]
fn malformed_numbers_and_truncated_headers() {
    assert_eq!(parse(b"P6\n"), Err(ConvertError::BadHeader));
    assert_eq!(parse(b"P6\nx 1\n255\n"), Err(ConvertError::BadHeader));
    assert_eq!(parse(b"P6\n1 1\n"), Err(ConvertError::BadHeader));
    assert_eq!(
        parse(b"P6\n99999999999 1\n255\n"),
        Err(ConvertError::BadHeader)
    );
    assert_eq!(parse(b"P6\n1 1\n255"), Err(ConvertError::BadHeader));
}

#[test]
fn unsupported_depth_and_maxval() {
    assert_eq!(
        parse(b"P6\n1 1\n65535\n\x00\x00\x00\x00\x00\x00"),
        Err(ConvertError::Unsupported)
    );
    assert_eq!(parse(b"P6\n1 1\n0\n\x00"), Err(ConvertError::Unsupported));
    let two = b"P7\nWIDTH 1\nHEIGHT 1\nDEPTH 2\nMAXVAL 255\nENDHDR\n\x00\x00";
    assert_eq!(parse(two), Err(ConvertError::Unsupported));
}

#[test]
fn bad_dimensions_never_allocate() {
    assert_eq!(parse(b"P6\n0 1\n255\n"), Err(ConvertError::BadDimensions));
    assert_eq!(parse(b"P6\n1 0\n255\n"), Err(ConvertError::BadDimensions));
    assert_eq!(
        parse(b"P6\n16385 1\n255\n"),
        Err(ConvertError::BadDimensions)
    );
    assert_eq!(
        parse(b"P6\n16384 16384\n255\n"),
        Err(ConvertError::BadPixelData)
    );
}

#[test]
fn pixel_data_must_match_exactly() {
    assert_eq!(
        parse(b"P6\n1 1\n255\n\x01\x02"),
        Err(ConvertError::BadPixelData)
    );
    assert_eq!(
        parse(b"P6\n1 1\n255\n\x01\x02\x03\x04"),
        Err(ConvertError::BadPixelData)
    );
}

fn pam(depth: u32, tuple: &str, pixels: &[u8]) -> Vec<u8> {
    let line = if tuple.is_empty() {
        String::new()
    } else {
        format!("TUPLTYPE {tuple}\n")
    };
    let mut bytes =
        format!("P7\nWIDTH 1\nHEIGHT 1\nDEPTH {depth}\nMAXVAL 255\n{line}ENDHDR\n").into_bytes();
    bytes.extend_from_slice(pixels);
    bytes
}

#[test]
fn p7_tuple_type_must_match_the_depth() {
    assert_eq!(
        parse(&pam(3, "RGB_ALPHA", &[1, 2, 3])),
        Err(ConvertError::Unsupported)
    );
    assert_eq!(
        parse(&pam(4, "RGB", &[1, 2, 3, 4])),
        Err(ConvertError::Unsupported)
    );
    assert_eq!(
        parse(&pam(3, "GRAYSCALE", &[1, 2, 3])),
        Err(ConvertError::Unsupported)
    );
    assert_eq!(
        parse(&pam(4, "GRAYSCALE_ALPHA", &[1, 2, 3, 4])),
        Err(ConvertError::Unsupported)
    );
}

#[test]
fn p7_matching_or_absent_tuple_type_is_accepted() {
    assert_eq!(
        must(parse(&pam(3, "RGB", &[1, 2, 3]))).rgba,
        vec![1, 2, 3, 255]
    );
    assert_eq!(
        must(parse(&pam(4, "RGB_ALPHA", &[1, 2, 3, 4]))).rgba,
        vec![1, 2, 3, 4]
    );
    assert_eq!(
        must(parse(&pam(3, "", &[1, 2, 3]))).rgba,
        vec![1, 2, 3, 255]
    );
}

#[test]
fn crlf_after_maxval_is_refused_not_misread() {
    // One whitespace byte ends the header; with CRLF the LF would be taken as pixel data, so the
    // length no longer matches. The file is rejected rather than decoded with shifted pixels.
    let mut bytes = b"P6\n1 1\n255\r\n".to_vec();
    bytes.extend_from_slice(&[1, 2, 3]);
    assert_eq!(parse(&bytes), Err(ConvertError::BadPixelData));
}

#[test]
fn no_token_at_the_end_of_the_data() {
    let mut tokens = Tokens {
        bytes: b"  # only a comment",
        pos: 0,
    };
    assert_eq!(tokens.token(), None);
    assert_eq!(Tokens { bytes: b"", pos: 0 }.number(), None);
    let mut two = Tokens {
        bytes: b" ab  7",
        pos: 0,
    };
    assert_eq!(two.token(), Some(&b"ab"[..]));
    assert_eq!(two.number(), Some(7));
    assert_eq!(two.token(), None);
}

#[test]
fn hostile_input_never_panics() {
    let mut base = b"P6\n2 2\n255\n".to_vec();
    base.extend_from_slice(&[7; 12]);
    for i in 0..base.len() {
        for value in [0u8, b'9', b'#', b' ', 0xFF] {
            let mut bytes = base.clone();
            bytes[i] = value;
            let _ = parse(&bytes);
            let _ = parse(&bytes[..i]);
        }
    }
}
