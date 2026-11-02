use super::*;
use assets::dtex::Texture;

fn must<T, E>(r: Result<T, E>) -> T {
    r.unwrap_or_else(|_| unreachable!())
}

fn ppm(width: u32, height: u32) -> Vec<u8> {
    let mut bytes = format!("P6\n{width} {height}\n255\n").into_bytes();
    bytes.extend(std::iter::repeat_n(
        50u8,
        usize::try_from(width * height * 3).unwrap_or(0),
    ));
    bytes
}

#[test]
fn converts_with_a_full_mip_chain() {
    let out = must(convert(
        &ppm(4, 2),
        Options {
            srgb: true,
            mips: true,
        },
    ));
    let tex = must(Texture::parse(&out));
    assert_eq!((tex.width(), tex.height(), tex.mip_count()), (4, 2, 3));
    assert!(tex.is_srgb());
    let last = tex.mip(2).unwrap_or_else(|| unreachable!());
    assert_eq!(last.bytes, &[50, 50, 50, 255]);
}

#[test]
fn no_mips_option_writes_one_level() {
    let out = must(convert(
        &ppm(4, 4),
        Options {
            srgb: false,
            mips: false,
        },
    ));
    let tex = must(Texture::parse(&out));
    assert_eq!(tex.mip_count(), 1);
    assert!(!tex.is_srgb());
}

#[test]
fn source_errors_pass_through() {
    let options = Options {
        srgb: false,
        mips: true,
    };
    assert_eq!(convert(b"nope", options), Err(ConvertError::UnknownFormat));
}

#[test]
fn conversion_is_deterministic() {
    let options = Options {
        srgb: false,
        mips: true,
    };
    assert_eq!(
        must(convert(&ppm(5, 3), options)),
        must(convert(&ppm(5, 3), options))
    );
}
