//! Key codes and the scancode tables of the two operating systems.

use input::{KeyCode, KeyMap};

#[test]
fn every_key_code_has_a_distinct_dense_index_that_fits_a_bitset() {
    assert!(KeyCode::ALL.len() <= 128);
    for (position, key) in KeyCode::ALL.iter().enumerate() {
        assert_eq!(key.index() as usize, position, "{key:?}");
    }
}

#[test]
fn each_table_names_every_key_once_and_every_scancode_once() {
    for map in [KeyMap::windows(), KeyMap::linux()] {
        assert_eq!(map.len(), KeyCode::ALL.len());
        for key in KeyCode::ALL {
            let codes: Vec<u32> = (0..0x200).filter(|c| map.key(*c) == Some(*key)).collect();
            assert_eq!(codes.len(), 1, "{key:?} has {codes:?}");
        }
    }
}

#[test]
fn the_windows_table_agrees_with_the_physics_sample() {
    let map = KeyMap::windows();
    assert_eq!(map.key(0x01), Some(KeyCode::Escape));
    assert_eq!(map.key(0x13), Some(KeyCode::R));
    assert_eq!(map.key(0x20), Some(KeyCode::D));
    assert_eq!(map.key(0x19), Some(KeyCode::P));
    assert_eq!(map.key(0x1F), Some(KeyCode::S));
}

#[test]
fn the_linux_table_agrees_with_the_physics_sample() {
    let map = KeyMap::linux();
    assert_eq!(map.key(9), Some(KeyCode::Escape));
    assert_eq!(map.key(27), Some(KeyCode::R));
    assert_eq!(map.key(40), Some(KeyCode::D));
    assert_eq!(map.key(33), Some(KeyCode::P));
    assert_eq!(map.key(39), Some(KeyCode::S));
}

#[test]
fn the_arrows_use_the_extended_codes_on_windows_and_the_evdev_codes_on_linux() {
    let (windows, linux) = (KeyMap::windows(), KeyMap::linux());
    assert_eq!(windows.key(0x148), Some(KeyCode::Up));
    assert_eq!(windows.key(0x150), Some(KeyCode::Down));
    assert_eq!(linux.key(103 + 8), Some(KeyCode::Up));
    assert_eq!(linux.key(108 + 8), Some(KeyCode::Down));
}

#[test]
fn an_unknown_scancode_has_no_key() {
    assert_eq!(KeyMap::windows().key(0xFFFF), None);
    assert_eq!(KeyMap::linux().key(0), None);
}

#[test]
fn the_host_table_is_the_one_of_the_operating_system() {
    let host = KeyMap::host();
    let own = if cfg!(windows) {
        KeyMap::windows()
    } else {
        KeyMap::linux()
    };
    for code in 0..0x200 {
        assert_eq!(host.key(code), own.key(code));
    }
}

#[test]
fn a_table_is_not_empty_and_an_empty_one_says_so() {
    assert!(!KeyMap::windows().is_empty());
    assert!(!KeyMap::linux().is_empty());
    let none = KeyMap::empty();
    assert!(none.is_empty());
    assert_eq!(none.len(), 0);
    assert_eq!(none.key(0x01), None);
}
