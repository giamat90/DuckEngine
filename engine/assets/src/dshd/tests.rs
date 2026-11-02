use super::*;

fn must<T, E>(r: Result<T, E>) -> T {
    r.unwrap_or_else(|_| unreachable!())
}

const VERTEX_CODE: &str = "#version 450\nvoid main() {}\n";
const FRAGMENT_CODE: &str = "#version 450\nvoid main() { }\n";

const INPUTS: [VertexInput; 3] = [
    VertexInput {
        location: 0,
        components: 2,
    },
    VertexInput {
        location: 1,
        components: 2,
    },
    VertexInput {
        location: 2,
        components: 4,
    },
];

fn vertex<'a>() -> ModuleSpec<'a> {
    ModuleSpec {
        stage: Stage::Vertex,
        uniform: Some(UniformBlock {
            binding: 0,
            size: 64,
        }),
        texture_binding: None,
        inputs: &INPUTS,
        code: VERTEX_CODE,
    }
}

fn fragment<'a>() -> ModuleSpec<'a> {
    ModuleSpec {
        stage: Stage::Fragment,
        uniform: None,
        texture_binding: Some(0),
        inputs: &[],
        code: FRAGMENT_CODE,
    }
}

fn encode(modules: &[ModuleSpec<'_>]) -> Vec<u8> {
    let mut out = vec![0u8; must(encoded_len(modules))];
    let written = must(encode_into(&mut out, modules));
    assert_eq!(written, out.len());
    out
}

#[test]
fn round_trip_of_a_vertex_and_fragment_pair() {
    let bytes = encode(&[vertex(), fragment()]);
    let program = must(Program::parse(&bytes));
    assert_eq!(program.module_count(), 2);
    let v = program
        .module_for(Stage::Vertex)
        .unwrap_or_else(|| unreachable!());
    assert_eq!(v.stage(), Stage::Vertex);
    assert_eq!(v.code(), VERTEX_CODE);
    assert_eq!(
        v.uniform(),
        Some(UniformBlock {
            binding: 0,
            size: 64
        })
    );
    assert_eq!(v.texture_binding(), None);
    assert_eq!(v.input_count(), 3);
    assert_eq!(v.inputs().collect::<Vec<_>>(), INPUTS.to_vec());
    let f = program
        .module_for(Stage::Fragment)
        .unwrap_or_else(|| unreachable!());
    assert_eq!(f.code(), FRAGMENT_CODE);
    assert_eq!(f.uniform(), None);
    assert_eq!(f.texture_binding(), Some(0));
    assert_eq!(f.input_count(), 0);
}

#[test]
fn module_index_follows_file_order() {
    let bytes = encode(&[fragment(), vertex()]);
    let program = must(Program::parse(&bytes));
    let first = program.module(0).unwrap_or_else(|| unreachable!());
    assert_eq!(first.stage(), Stage::Fragment);
    let second = program.module(1).unwrap_or_else(|| unreachable!());
    assert_eq!(second.stage(), Stage::Vertex);
    assert!(program.module(2).is_none());
}

#[test]
fn a_single_module_is_a_valid_program() {
    let bytes = encode(&[vertex()]);
    let program = must(Program::parse(&bytes));
    assert_eq!(program.module_count(), 1);
    assert!(program.module_for(Stage::Fragment).is_none());
}

#[test]
fn header_and_record_layout_is_fixed() {
    let bytes = encode(&[fragment()]);
    assert_eq!(&bytes[0..4], b"DSHD");
    assert_eq!(bytes[4..6], 0u16.to_le_bytes());
    assert_eq!(bytes[6], 0, "backend: GLSL 450");
    assert_eq!(bytes[7], 1, "module count");
    assert_eq!(HEADER_LEN, 8);
    let record = &bytes[HEADER_LEN..];
    assert_eq!(record[0], 1, "stage: fragment");
    assert_eq!(record[1], 0, "input count");
    assert_eq!(record[2], NO_BINDING, "no uniform block");
    assert_eq!(record[3], 0, "texture binding");
    assert_eq!(record[4..8], 0u32.to_le_bytes(), "uniform size");
    assert_eq!(
        record[8..12],
        u32::try_from(FRAGMENT_CODE.len())
            .unwrap_or(0)
            .to_le_bytes()
    );
    assert_eq!(record[12..16], [0, 0, 0, 0]);
    assert_eq!(RECORD_LEN, 16);
    assert_eq!(&record[16..], FRAGMENT_CODE.as_bytes());
}

#[test]
fn input_entries_follow_the_record() {
    let bytes = encode(&[vertex()]);
    let at = HEADER_LEN + RECORD_LEN;
    assert_eq!(bytes[at..at + 6], [0, 2, 1, 2, 2, 4]);
}

#[test]
fn encoder_refuses_what_the_reader_would_refuse() {
    let mut m = vertex();
    m.code = "";
    assert_eq!(encoded_len(&[m]), Err(DshdError::EmptyCode));
    assert_eq!(encoded_len(&[]), Err(DshdError::BadModuleCount));
    assert_eq!(
        encoded_len(&[vertex(), vertex()]),
        Err(DshdError::DuplicateStage)
    );
    assert_eq!(
        encoded_len(&[vertex(), fragment(), vertex()]),
        Err(DshdError::BadModuleCount)
    );
    let mut frag_with_inputs = fragment();
    frag_with_inputs.inputs = &INPUTS;
    assert_eq!(encoded_len(&[frag_with_inputs]), Err(DshdError::BadInputs));
    let unordered = [INPUTS[1], INPUTS[0]];
    let mut v = vertex();
    v.inputs = &unordered;
    assert_eq!(encoded_len(&[v]), Err(DshdError::BadInputs));
    let wide = [VertexInput {
        location: 0,
        components: 5,
    }];
    let mut v = vertex();
    v.inputs = &wide;
    assert_eq!(encoded_len(&[v]), Err(DshdError::BadInputs));
    let zero = [VertexInput {
        location: 0,
        components: 0,
    }];
    let mut v = vertex();
    v.inputs = &zero;
    assert_eq!(encoded_len(&[v]), Err(DshdError::BadInputs));
    let mut v = vertex();
    v.uniform = Some(UniformBlock {
        binding: 0,
        size: 0,
    });
    assert_eq!(encoded_len(&[v]), Err(DshdError::BadBinding));
    let mut v = vertex();
    v.uniform = Some(UniformBlock {
        binding: NO_BINDING,
        size: 64,
    });
    assert_eq!(encoded_len(&[v]), Err(DshdError::BadBinding));
    let mut f = fragment();
    f.texture_binding = Some(NO_BINDING);
    assert_eq!(encoded_len(&[f]), Err(DshdError::BadBinding));
}

#[test]
fn too_many_inputs_are_refused() {
    let many: Vec<VertexInput> = (0..=u8::try_from(MAX_INPUTS).unwrap_or(0))
        .map(|location| VertexInput {
            location,
            components: 1,
        })
        .collect();
    assert_eq!(many.len(), MAX_INPUTS + 1);
    let mut v = vertex();
    v.inputs = &many;
    assert_eq!(encoded_len(&[v]), Err(DshdError::BadInputs));
}

#[test]
fn code_longer_than_the_limit_is_refused() {
    let long = "x".repeat(MAX_CODE_LEN + 1);
    let mut v = vertex();
    v.code = &long;
    assert_eq!(encoded_len(&[v]), Err(DshdError::CodeTooLong));
    let max = "y".repeat(MAX_CODE_LEN);
    let mut v = vertex();
    v.code = &max;
    assert!(encoded_len(&[v]).is_ok());
}

#[test]
fn encode_into_a_small_buffer_is_truncated() {
    let len = must(encoded_len(&[vertex(), fragment()]));
    let mut small = vec![0u8; len - 1];
    assert_eq!(
        encode_into(&mut small, &[vertex(), fragment()]),
        Err(DshdError::Truncated)
    );
    let mut exact = vec![0u8; len];
    assert_eq!(encode_into(&mut exact, &[vertex(), fragment()]), Ok(len));
}

#[test]
fn every_prefix_of_a_valid_file_is_refused() {
    let bytes = encode(&[vertex(), fragment()]);
    for cut in 0..bytes.len() {
        assert!(Program::parse(&bytes[..cut]).is_err(), "prefix {cut}");
    }
}

#[test]
fn trailing_bytes_are_refused() {
    let mut bytes = encode(&[vertex(), fragment()]);
    bytes.push(0);
    assert_eq!(Program::parse(&bytes), Err(DshdError::TrailingData));
}

fn patched(at: usize, value: u8) -> Vec<u8> {
    let mut bytes = encode(&[vertex(), fragment()]);
    bytes[at] = value;
    bytes
}

#[test]
fn header_fields_are_checked() {
    assert_eq!(Program::parse(&patched(0, b'X')), Err(DshdError::BadMagic));
    assert_eq!(
        Program::parse(&patched(4, 1)),
        Err(DshdError::UnsupportedVersion(1))
    );
    assert_eq!(
        Program::parse(&patched(6, 9)),
        Err(DshdError::UnsupportedBackend(9))
    );
    assert_eq!(
        Program::parse(&patched(7, 0)),
        Err(DshdError::BadModuleCount)
    );
    assert_eq!(
        Program::parse(&patched(7, 3)),
        Err(DshdError::BadModuleCount)
    );
}

#[test]
fn record_fields_are_checked() {
    let rec = HEADER_LEN;
    assert_eq!(
        Program::parse(&patched(rec, 7)),
        Err(DshdError::BadStage(7))
    );
    assert_eq!(
        Program::parse(&patched(rec + 12, 1)),
        Err(DshdError::ReservedNonZero)
    );
    assert_eq!(
        Program::parse(&patched(rec + 1, 200)),
        Err(DshdError::BadInputs)
    );
    assert_eq!(
        Program::parse(&patched(rec + 2, NO_BINDING)),
        Err(DshdError::BadBinding)
    );
}

#[test]
fn a_second_module_of_the_same_stage_is_refused() {
    let mut bytes = encode(&[vertex(), fragment()]);
    let second = HEADER_LEN + RECORD_LEN + INPUTS.len() * 2 + VERTEX_CODE.len();
    bytes[second] = 0;
    assert_eq!(Program::parse(&bytes), Err(DshdError::DuplicateStage));
}

#[test]
fn code_that_is_not_utf8_is_refused() {
    let mut bytes = encode(&[fragment()]);
    let at = HEADER_LEN + RECORD_LEN;
    bytes[at] = 0xFF;
    assert_eq!(Program::parse(&bytes), Err(DshdError::BadCode));
}

#[test]
fn a_code_length_beyond_the_file_is_truncated_not_a_panic() {
    let mut bytes = encode(&[fragment()]);
    let at = HEADER_LEN + 8;
    bytes[at..at + 4].copy_from_slice(&u32::MAX.to_le_bytes());
    assert!(Program::parse(&bytes).is_err());
}

#[test]
fn every_error_has_its_own_message() {
    let all = [
        DshdError::Truncated,
        DshdError::BadMagic,
        DshdError::UnsupportedVersion(1),
        DshdError::UnsupportedBackend(1),
        DshdError::BadModuleCount,
        DshdError::BadStage(1),
        DshdError::DuplicateStage,
        DshdError::BadInputs,
        DshdError::BadBinding,
        DshdError::EmptyCode,
        DshdError::CodeTooLong,
        DshdError::BadCode,
        DshdError::ReservedNonZero,
        DshdError::TrailingData,
    ];
    let texts: Vec<String> = all.iter().map(ToString::to_string).collect();
    for (i, a) in texts.iter().enumerate() {
        assert!(!a.is_empty());
        assert!(texts[i + 1..].iter().all(|b| a != b));
    }
}

fn inputs_at() -> usize {
    HEADER_LEN + RECORD_LEN
}

#[test]
fn encoder_refuses_two_inputs_at_one_location() {
    let same = [
        VertexInput {
            location: 3,
            components: 2,
        },
        VertexInput {
            location: 3,
            components: 2,
        },
    ];
    let mut v = vertex();
    v.inputs = &same;
    assert_eq!(encoded_len(&[v]), Err(DshdError::BadInputs));
}

#[test]
fn sixteen_inputs_round_trip_and_seventeen_do_not() {
    let sixteen: Vec<VertexInput> = (0..16u8)
        .map(|location| VertexInput {
            location,
            components: 1,
        })
        .collect();
    let mut v = vertex();
    v.inputs = &sixteen;
    let bytes = encode(&[v]);
    let program = must(Program::parse(&bytes));
    let module = program.module(0).unwrap_or_else(|| unreachable!());
    assert_eq!(module.input_count(), 16);
    assert_eq!(module.inputs().collect::<Vec<_>>(), sixteen);
    let mut seventeen_bytes = bytes;
    seventeen_bytes[HEADER_LEN + 1] = 17;
    assert_eq!(Program::parse(&seventeen_bytes), Err(DshdError::BadInputs));
}

#[test]
fn reader_refuses_inputs_in_a_fragment_module() {
    let mut bytes = encode(&[fragment()]);
    bytes[HEADER_LEN + 1] = 1;
    let at = inputs_at();
    bytes.splice(at..at, [0u8, 2u8]);
    assert_eq!(Program::parse(&bytes), Err(DshdError::BadInputs));
}

#[test]
fn reader_refuses_bad_input_entries() {
    let base = encode(&[vertex()]);
    let at = inputs_at();
    let mut equal = base.clone();
    equal[at + 2] = equal[at];
    assert_eq!(Program::parse(&equal), Err(DshdError::BadInputs));
    let mut down = base.clone();
    down[at] = 9;
    assert_eq!(Program::parse(&down), Err(DshdError::BadInputs));
    let mut zero = base.clone();
    zero[at + 1] = 0;
    assert_eq!(Program::parse(&zero), Err(DshdError::BadInputs));
    let mut five = base.clone();
    five[at + 3] = 5;
    assert_eq!(Program::parse(&five), Err(DshdError::BadInputs));
    let mut both = base;
    both[at] = 9;
    both[at + 1] = 7;
    assert_eq!(Program::parse(&both), Err(DshdError::BadInputs));
}

#[test]
fn reader_checks_the_block_binding_against_its_size() {
    let mut size_without_binding = encode(&[vertex()]);
    size_without_binding[HEADER_LEN + 2] = NO_BINDING;
    assert_eq!(
        Program::parse(&size_without_binding),
        Err(DshdError::BadBinding)
    );
    let mut binding_without_size = encode(&[fragment()]);
    binding_without_size[HEADER_LEN + 2] = 0;
    assert_eq!(
        Program::parse(&binding_without_size),
        Err(DshdError::BadBinding)
    );
}

#[test]
fn reader_limits_the_code_length() {
    let max = "z".repeat(MAX_CODE_LEN);
    let mut f = fragment();
    f.code = &max;
    let bytes = encode(&[f]);
    let program = must(Program::parse(&bytes));
    assert_eq!(
        program.module(0).map(|m| m.code().len()),
        Some(MAX_CODE_LEN)
    );
    let at = HEADER_LEN + 8;
    let mut over = encode(&[fragment()]);
    let too_long = u32::try_from(MAX_CODE_LEN).unwrap_or(0) + 1;
    over[at..at + 4].copy_from_slice(&too_long.to_le_bytes());
    assert_eq!(Program::parse(&over), Err(DshdError::CodeTooLong));
    let mut empty = encode(&[fragment()]);
    empty[at..at + 4].copy_from_slice(&0u32.to_le_bytes());
    assert_eq!(Program::parse(&empty), Err(DshdError::EmptyCode));
}
