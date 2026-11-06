//! The parsing and the variance rule of the instruction-count tooling (card 4-22). The sample
//! texts are the formats of valgrind and perf as documented; the hosted-runner outputs the probe
//! printed are in `docs/budgets/physics-benchmark-4-22.md`.

use tools_physics_bench::{
    GateFeasibility, IcountError, Tool, gate_feasibility, parse_instructions, relative_spread,
};

#[test]
fn the_lines_valgrind_printed_on_a_hosted_runner_are_read() {
    // Copied from the log of the probe run on ubuntu-26.04 (valgrind 3.26.0): cachegrind writes a
    // single space after `I`.
    let callgrind = "==3037== Collected : 162566311\n==3037== \n==3037== Events    : Ir\n";
    assert_eq!(
        parse_instructions(Tool::Callgrind, callgrind),
        Ok(162_566_311)
    );
    let cachegrind = "==3066== \n==3066== I refs:        163,284,496\n";
    assert_eq!(
        parse_instructions(Tool::Cachegrind, cachegrind),
        Ok(163_284_496)
    );
}

#[test]
fn callgrind_prints_the_total_on_its_collected_line() {
    let text = "==41== Events    : Ir\n==41== Collected : 1234567890\n==41==\n==41== I   refs:      1,234,567,890\n";
    assert_eq!(parse_instructions(Tool::Callgrind, text), Ok(1_234_567_890));
}

#[test]
fn callgrind_without_a_collected_line_is_malformed() {
    assert_eq!(
        parse_instructions(Tool::Callgrind, "==41== nothing\n"),
        Err(IcountError::Malformed)
    );
}

#[test]
fn cachegrind_prints_instruction_references_with_thousands_separators() {
    let text = "==7== I   refs:      2,000,000\n==7== I1  misses:        3,001\n";
    assert_eq!(parse_instructions(Tool::Cachegrind, text), Ok(2_000_000));
}

#[test]
fn cachegrind_does_not_mistake_a_miss_line_for_the_total() {
    let text = "==7== I1  misses:        3,001\n==7== LLi misses:          12\n";
    assert_eq!(
        parse_instructions(Tool::Cachegrind, text),
        Err(IcountError::Malformed)
    );
}

#[test]
fn perf_stat_default_output_names_the_instruction_event() {
    let text = " Performance counter stats for './bench':\n\n     1,500,000,000      instructions:u            #    1.20  insn per cycle\n";
    assert_eq!(parse_instructions(Tool::PerfStat, text), Ok(1_500_000_000));
}

#[test]
fn perf_stat_csv_output_is_read_too() {
    let text = "1500000000,,instructions:u,1234567,100.00,1.20,insn per cycle\n";
    assert_eq!(parse_instructions(Tool::PerfStat, text), Ok(1_500_000_000));
}

#[test]
fn perf_stat_on_a_virtual_machine_says_the_counter_is_not_supported() {
    let text = "     <not supported>      instructions:u\n";
    assert_eq!(
        parse_instructions(Tool::PerfStat, text),
        Err(IcountError::CounterUnavailable)
    );
    let csv = "<not counted>,,instructions:u,0,100.00,,\n";
    assert_eq!(
        parse_instructions(Tool::PerfStat, csv),
        Err(IcountError::CounterUnavailable)
    );
}

#[test]
fn the_tool_names_are_read_from_the_command_line() {
    assert_eq!("callgrind".parse(), Ok(Tool::Callgrind));
    assert_eq!("cachegrind".parse(), Ok(Tool::Cachegrind));
    assert_eq!("perf".parse(), Ok(Tool::PerfStat));
    assert_eq!("gdb".parse::<Tool>(), Err(IcountError::UnknownTool));
}

#[test]
fn identical_counts_have_no_spread() {
    assert_eq!(relative_spread(&[1000, 1000, 1000]), Some(0.0));
}

#[test]
fn the_spread_is_the_range_over_the_smallest_count() {
    let spread = relative_spread(&[1000, 1010, 1005]).unwrap_or(f64::NAN);
    assert!((spread - 0.01).abs() < 1e-12, "{spread}");
}

#[test]
fn a_spread_needs_two_counts_and_a_smallest_above_zero() {
    assert_eq!(relative_spread(&[]), None);
    assert_eq!(relative_spread(&[5]), None);
    assert_eq!(relative_spread(&[0, 5]), None);
}

#[test]
fn two_counts_are_enough_for_a_spread() {
    assert_eq!(relative_spread(&[100, 100]), Some(0.0));
    assert_eq!(relative_spread(&[100, 150]), Some(0.5));
}

#[test]
fn counts_above_32_bits_keep_both_halves() {
    // low = 2^32 + 1, high = 2^33 + 2: the range equals the low count, so the spread is exactly 1
    let low = (1_u64 << 32) + 1;
    let high = (1_u64 << 33) + 2;
    assert_eq!(relative_spread(&[low, high]), Some(1.0));
    // only the low half differs: 3 / 2^32, checked to a relative 1e-12 so a wrong 2^32 shows
    let expected = 3.0 / 4_294_967_296.0;
    let spread = relative_spread(&[1 << 32, (1 << 32) + 3]).unwrap_or(f64::NAN);
    assert!((spread - expected).abs() < expected * 1e-12, "{spread}");
}

#[test]
fn a_gate_is_feasible_when_the_noise_is_a_tenth_of_the_warn_band_or_less() {
    // warn band 5 %: the noise must be at most 0.5 %
    assert_eq!(gate_feasibility(0.0), GateFeasibility::Feasible);
    assert_eq!(gate_feasibility(0.005), GateFeasibility::Feasible);
    assert_eq!(gate_feasibility(0.02), GateFeasibility::Marginal);
    assert_eq!(gate_feasibility(0.05), GateFeasibility::Marginal);
    assert_eq!(gate_feasibility(0.0501), GateFeasibility::Infeasible);
}
