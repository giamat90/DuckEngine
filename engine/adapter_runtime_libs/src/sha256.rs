//! SHA-256 (FIPS 180-4), written here so that the build helper needs no crate. Checked against the
//! standard test vectors in `tests`.

use std::fs::File;
use std::io::{self, Read};
use std::path::Path;

const K: [u32; 64] = [
    0x428a_2f98,
    0x7137_4491,
    0xb5c0_fbcf,
    0xe9b5_dba5,
    0x3956_c25b,
    0x59f1_11f1,
    0x923f_82a4,
    0xab1c_5ed5,
    0xd807_aa98,
    0x1283_5b01,
    0x2431_85be,
    0x550c_7dc3,
    0x72be_5d74,
    0x80de_b1fe,
    0x9bdc_06a7,
    0xc19b_f174,
    0xe49b_69c1,
    0xefbe_4786,
    0x0fc1_9dc6,
    0x240c_a1cc,
    0x2de9_2c6f,
    0x4a74_84aa,
    0x5cb0_a9dc,
    0x76f9_88da,
    0x983e_5152,
    0xa831_c66d,
    0xb003_27c8,
    0xbf59_7fc7,
    0xc6e0_0bf3,
    0xd5a7_9147,
    0x06ca_6351,
    0x1429_2967,
    0x27b7_0a85,
    0x2e1b_2138,
    0x4d2c_6dfc,
    0x5338_0d13,
    0x650a_7354,
    0x766a_0abb,
    0x81c2_c92e,
    0x9272_2c85,
    0xa2bf_e8a1,
    0xa81a_664b,
    0xc24b_8b70,
    0xc76c_51a3,
    0xd192_e819,
    0xd699_0624,
    0xf40e_3585,
    0x106a_a070,
    0x19a4_c116,
    0x1e37_6c08,
    0x2748_774c,
    0x34b0_bcb5,
    0x391c_0cb3,
    0x4ed8_aa4a,
    0x5b9c_ca4f,
    0x682e_6ff3,
    0x748f_82ee,
    0x78a5_636f,
    0x84c8_7814,
    0x8cc7_0208,
    0x90be_fffa,
    0xa450_6ceb,
    0xbef9_a3f7,
    0xc671_78f2,
];

const START: [u32; 8] = [
    0x6a09_e667,
    0xbb67_ae85,
    0x3c6e_f372,
    0xa54f_f53a,
    0x510e_527f,
    0x9b05_688c,
    0x1f83_d9ab,
    0x5be0_cd19,
];

const BLOCK: usize = 64;

/// A running SHA-256.
pub struct Sha256 {
    state: [u32; 8],
    buffer: [u8; BLOCK],
    filled: usize,
    length: u64,
}

impl Default for Sha256 {
    fn default() -> Self {
        Self::new()
    }
}

impl Sha256 {
    /// A hash of nothing yet.
    #[must_use]
    pub fn new() -> Self {
        Self {
            state: START,
            buffer: [0; BLOCK],
            filled: 0,
            length: 0,
        }
    }

    /// Adds `data`.
    pub fn update(&mut self, data: &[u8]) {
        self.length = self
            .length
            .wrapping_add(u64::try_from(data.len()).unwrap_or(u64::MAX));
        let mut rest = data;
        while !rest.is_empty() {
            rest = self.fill(rest);
        }
    }

    /// Copies as much of `data` as fits in the block, compresses a full block, returns the rest.
    fn fill<'a>(&mut self, data: &'a [u8]) -> &'a [u8] {
        let take = (BLOCK - self.filled).min(data.len());
        let (now, later) = data.split_at(take);
        self.buffer[self.filled..self.filled + take].copy_from_slice(now);
        self.filled += take;
        if self.filled == BLOCK {
            compress(&mut self.state, &self.buffer);
            self.filled = 0;
        }
        later
    }

    /// The 32 bytes of the hash.
    #[must_use]
    pub fn finish(mut self) -> [u8; 32] {
        let bits = self.length.wrapping_mul(8);
        self.update_padding(&[0x80]);
        while self.filled != BLOCK - 8 {
            self.update_padding(&[0]);
        }
        self.update_padding(&bits.to_be_bytes());
        let mut out = [0_u8; 32];
        for (word, bytes) in self.state.iter().zip(out.chunks_exact_mut(4)) {
            bytes.copy_from_slice(&word.to_be_bytes());
        }
        out
    }

    /// Adds padding bytes without counting them in the length.
    fn update_padding(&mut self, data: &[u8]) {
        let length = self.length;
        self.update(data);
        self.length = length;
    }
}

fn compress(state: &mut [u32; 8], block: &[u8; BLOCK]) {
    let mut schedule = [0_u32; 64];
    for (word, bytes) in schedule.iter_mut().zip(block.chunks_exact(4)) {
        *word = u32::from_be_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]);
    }
    for i in 16..64 {
        let early = schedule[i - 15];
        let late = schedule[i - 2];
        let small0 = early.rotate_right(7) ^ early.rotate_right(18) ^ (early >> 3);
        let small1 = late.rotate_right(17) ^ late.rotate_right(19) ^ (late >> 10);
        schedule[i] = schedule[i - 16]
            .wrapping_add(small0)
            .wrapping_add(schedule[i - 7])
            .wrapping_add(small1);
    }
    let mut v = *state;
    for (constant, word) in K.iter().zip(schedule) {
        v = round(v, *constant, word);
    }
    for (slot, value) in state.iter_mut().zip(v) {
        *slot = slot.wrapping_add(value);
    }
}

/// One round on the eight working variables, in the order a, b, c, d, e, f, g, h of FIPS 180-4.
fn round(v: [u32; 8], constant: u32, word: u32) -> [u32; 8] {
    let [va, vb, vc, vd, ve, vf, vg, vh] = v;
    let sum1 = ve.rotate_right(6) ^ ve.rotate_right(11) ^ ve.rotate_right(25);
    let choose = (ve & vf) ^ (!ve & vg);
    let first = vh
        .wrapping_add(sum1)
        .wrapping_add(choose)
        .wrapping_add(constant)
        .wrapping_add(word);
    let sum0 = va.rotate_right(2) ^ va.rotate_right(13) ^ va.rotate_right(22);
    let majority = (va & vb) ^ (va & vc) ^ (vb & vc);
    [
        first.wrapping_add(sum0.wrapping_add(majority)),
        va,
        vb,
        vc,
        vd.wrapping_add(first),
        ve,
        vf,
        vg,
    ]
}

/// Lower-case hex of `bytes`.
#[must_use]
pub fn hex(bytes: &[u8]) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut text = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        text.push(char::from(DIGITS[usize::from(byte >> 4)]));
        text.push(char::from(DIGITS[usize::from(byte & 15)]));
    }
    text
}

/// The SHA-256 of `data` as 64 lower-case hex digits.
#[must_use]
pub fn sha256_hex(data: &[u8]) -> String {
    let mut hash = Sha256::new();
    hash.update(data);
    hex(&hash.finish())
}

/// The SHA-256 of the file at `path`, read in pieces.
///
/// # Errors
/// The error of opening or reading the file.
pub fn file_sha256_hex(path: &Path) -> io::Result<String> {
    let mut file = File::open(path)?;
    let mut hash = Sha256::new();
    let mut piece = [0_u8; 1 << 12];
    loop {
        let read = file.read(&mut piece)?;
        if read == 0 {
            return Ok(hex(&hash.finish()));
        }
        hash.update(&piece[..read]);
    }
}
