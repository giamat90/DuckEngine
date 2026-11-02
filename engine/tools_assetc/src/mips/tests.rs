use super::*;

#[test]
fn averages_a_two_by_two_block_with_rounding() {
    // Channel 0: 0, 1, 1, 1 -> sum 3 -> (3 + 2) / 4 = 1; channel 1: 10, 20, 30, 40 -> 25.
    let src = [0, 10, 0, 255, 1, 20, 0, 255, 1, 30, 0, 255, 1, 40, 0, 255];
    assert_eq!(box_down(&src, (2, 2), (1, 1)), vec![1, 25, 0, 255]);
}

#[test]
fn halves_a_row_texture() {
    let src = [0, 0, 0, 0, 4, 4, 4, 4, 8, 8, 8, 8, 12, 12, 12, 12];
    // Height 1 stays 1: the clamped row repeats, so each output pixel averages two columns.
    assert_eq!(
        box_down(&src, (4, 1), (2, 1)),
        vec![2, 2, 2, 2, 10, 10, 10, 10]
    );
}

#[test]
fn odd_source_clamps_at_the_edge() {
    let src = [0, 0, 0, 0, 10, 10, 10, 10, 200, 200, 200, 200];
    // 3 wide to 1 wide: averages columns 0 and 1 only (floor-sized chain).
    assert_eq!(box_down(&src, (3, 1), (1, 1)), vec![5, 5, 5, 5]);
}

#[test]
fn odd_dimensions_drop_the_last_column_and_row() {
    // 3x3, red channel: rows 0 1 2 / 3 4 5 / 6 7 8 times 10. A 1x1 target averages the top-left
    // 2x2 block (0, 10, 30, 40 -> 20); the last column (20, 50, 80) and row (60, 70, 80) are dropped.
    let src: Vec<u8> = (0..9u8).flat_map(|i| [i * 10, 0, 0, 0]).collect();
    assert_eq!(box_down(&src, (3, 3), (1, 1)), vec![20, 0, 0, 0]);
    // 5x3 to 2x1: blocks at columns 0-1 and 2-3, rows 0-1; column 4 and row 2 are not read.
    let wide: Vec<u8> = (0..15u8).flat_map(|i| [i * 10, 0, 0, 0]).collect();
    assert_eq!(
        box_down(&wide, (5, 3), (2, 1)),
        vec![30, 0, 0, 0, 50, 0, 0, 0]
    );
}

#[test]
fn one_by_one_stays_itself() {
    assert_eq!(box_down(&[9, 8, 7, 6], (1, 1), (1, 1)), vec![9, 8, 7, 6]);
}

#[test]
fn output_length_follows_the_destination_size() {
    let src = vec![0u8; 8 * 4 * 4];
    assert_eq!(box_down(&src, (8, 4), (4, 2)).len(), 4 * 2 * 4);
}

#[test]
fn a_short_source_reads_as_zero_not_a_panic() {
    assert_eq!(box_down(&[255, 255, 255, 255], (2, 2), (1, 1)).len(), 4);
}

#[test]
fn an_empty_source_reads_as_zero_not_a_panic() {
    assert_eq!(box_down(&[], (0, 0), (1, 1)), vec![0, 0, 0, 0]);
}

#[test]
fn chain_has_every_level() {
    let base = vec![100u8; 4 * 4 * 4];
    let chain = build_chain(base, (4, 4), 3);
    let lens: Vec<usize> = chain.iter().map(Vec::len).collect();
    assert_eq!(lens, vec![64, 16, 4]);
    assert!(chain[2].iter().all(|&b| b == 100));
}

/// A 4x4 image whose red channel is `10 * (row * 4 + column)`; other channels 0.
fn gradient() -> Vec<u8> {
    (0..16u8).flat_map(|i| [i * 10, 0, 0, 0]).collect()
}

#[test]
fn rows_are_averaged_too() {
    // 2 wide, 4 tall -> 1 wide, 2 tall: row pairs (0,1) and (2,3).
    let src = [
        0, 0, 0, 0, 0, 0, 0, 0, 40, 0, 0, 0, 40, 0, 0, 0, 80, 0, 0, 0, 80, 0, 0, 0, 120, 0, 0, 0,
        120, 0, 0, 0,
    ];
    assert_eq!(
        box_down(&src, (2, 4), (1, 2)),
        vec![20, 0, 0, 0, 100, 0, 0, 0]
    );
}

#[test]
fn each_level_is_built_from_the_one_before() {
    let chain = build_chain(gradient(), (4, 4), 3);
    assert_eq!(chain[1], box_down(&chain[0], (4, 4), (2, 2)));
    assert_eq!(chain[2], box_down(&chain[1], (2, 2), (1, 1)));
    // Level 1 of the gradient: block means 25, 45, 105, 125; level 2 is their mean, 75.
    let reds: Vec<u8> = chain[1].chunks(4).map(|p| p[0]).collect();
    assert_eq!(reds, vec![25, 45, 105, 125]);
    assert_eq!(chain[2][0], 75);
}
