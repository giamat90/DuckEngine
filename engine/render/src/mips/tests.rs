use super::*;

#[test]
fn level_zero_is_the_full_size_and_each_level_halves_down_to_one_texel() {
    assert_eq!(level_size(8, 4, 0), (8, 4));
    assert_eq!(level_size(8, 4, 1), (4, 2));
    assert_eq!(level_size(8, 4, 2), (2, 1));
    assert_eq!(level_size(8, 4, 3), (1, 1));
    // An odd size rounds down; the chain never goes below one texel, however far the level.
    assert_eq!(level_size(5, 3, 1), (2, 1));
    assert_eq!(level_size(5, 3, 2), (1, 1));
    assert_eq!(level_size(5, 3, 40), (1, 1));
}

#[test]
fn the_full_chain_has_a_level_for_every_halving_of_the_longer_side() {
    assert_eq!(level_count(1, 1), 1);
    assert_eq!(level_count(4, 2), 3);
    assert_eq!(level_count(5, 3), 3);
    assert_eq!(level_count(64, 64), 7);
    assert_eq!(level_count(1, 256), 9);
}

#[test]
fn the_last_level_of_the_full_chain_is_one_texel() {
    for (w, h) in [(1, 1), (4, 2), (5, 3), (64, 64), (1, 256), (100, 37)] {
        assert_eq!(level_size(w, h, level_count(w, h) - 1), (1, 1), "{w}x{h}");
    }
}

fn texels(values: &[[u8; 4]]) -> Vec<u8> {
    values.iter().flatten().copied().collect()
}

#[test]
fn a_two_by_two_averages_to_one_texel() {
    let level = texels(&[
        [0, 0, 0, 255],
        [100, 40, 8, 255],
        [200, 0, 0, 255],
        [100, 80, 4, 255],
    ]);
    assert_eq!(downsample(&level, 2, 2), Some(vec![100, 30, 3, 255]));
}

#[test]
fn a_checker_averages_to_the_middle_value_and_keeps_the_size_arithmetic() {
    let (dark, light) = ([0, 0, 0, 255], [200, 200, 200, 255]);
    let level: Vec<u8> = (0..16_usize)
        .flat_map(|i| {
            if (i % 4 + i / 4) % 2 == 0 {
                dark
            } else {
                light
            }
        })
        .collect();
    let next = downsample(&level, 4, 4);
    assert_eq!(next, Some(texels(&[[100, 100, 100, 255]; 4])));
}

#[test]
fn an_odd_size_drops_the_last_row_and_column_like_the_level_size_says() {
    // 3x3 -> 1x1: the texel is the average of the top-left 2x2 only.
    let mut level = vec![0_u8; 3 * 3 * 4];
    for (i, texel) in level.chunks_exact_mut(4).enumerate() {
        let value = u8::try_from(i * 10).unwrap_or(u8::MAX);
        texel.copy_from_slice(&[value, value, value, 255]);
    }
    // texels 0, 1, 3, 4 are the top-left 2x2: (0 + 10 + 30 + 40) / 4 = 20.
    assert_eq!(downsample(&level, 3, 3), Some(vec![20, 20, 20, 255]));
}

#[test]
fn a_size_of_one_in_a_direction_averages_along_the_other_only() {
    let level = texels(&[
        [10, 0, 0, 255],
        [30, 0, 0, 255],
        [50, 0, 0, 255],
        [70, 0, 0, 255],
    ]);
    assert_eq!(
        downsample(&level, 4, 1),
        Some(texels(&[[20, 0, 0, 255], [60, 0, 0, 255]]))
    );
}

#[test]
fn a_wrong_byte_count_or_a_zero_size_gives_nothing() {
    assert_eq!(downsample(&[0; 15], 2, 2), None);
    assert_eq!(downsample(&[0; 17], 2, 2), None);
    assert_eq!(downsample(&[], 0, 4), None);
    assert_eq!(
        downsample(&[0; 4], 1, 1),
        None,
        "one texel has no next level"
    );
}

#[test]
fn the_chain_runs_from_the_given_level_down_to_one_texel() {
    let level0 = texels(&[[8; 4]; 8]); // 4x2
    let chain = mip_chain(&level0, 4, 2);
    let sizes: Option<Vec<usize>> = chain.as_ref().map(|c| c.iter().map(Vec::len).collect());
    assert_eq!(sizes, Some(vec![32, 8, 4]));
    assert_eq!(
        chain.as_ref().map(Vec::len),
        Some(level_count(4, 2) as usize)
    );
    assert_eq!(chain.as_ref().and_then(|c| c.first()), Some(&level0));
    assert_eq!(
        chain.as_ref().and_then(|c| c.last()),
        Some(&vec![8, 8, 8, 8])
    );
}

#[test]
fn a_chain_of_a_wrong_sized_image_is_none() {
    assert_eq!(mip_chain(&[0; 7], 2, 1), None);
    assert_eq!(mip_chain(&[0; 7], 1, 1), None);
    assert_eq!(mip_chain(&[], 0, 4), None);
    assert_eq!(mip_chain(&[0; 4], 1, 1), Some(vec![vec![0; 4]]));
}
