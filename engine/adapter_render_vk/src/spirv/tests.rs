use super::*;

const HEADER: [u32; 5] = [MAGIC, 0x0001_0000, 0, 64, 0];

fn bytes(words: &[u32]) -> Vec<u8> {
    words.iter().flat_map(|word| word.to_le_bytes()).collect()
}

fn instruction(opcode: u32, operands: &[u32]) -> Vec<u32> {
    let length = u32::try_from(operands.len() + 1).unwrap_or(0);
    let mut words = vec![(length << 16) | opcode];
    words.extend_from_slice(operands);
    words
}

/// A module with one variable `id` of `class`, decorated with `set` and `binding`.
fn module(id: u32, class: u32, set: u32, binding: u32) -> Vec<u32> {
    let mut words = HEADER.to_vec();
    words.extend(instruction(
        OP_DECORATE,
        &[id, DECORATION_DESCRIPTOR_SET, set],
    ));
    words.extend(instruction(OP_DECORATE, &[id, DECORATION_BINDING, binding]));
    words.extend(instruction(OP_VARIABLE, &[9, id, class]));
    words
}

fn set_of(words: &[u32]) -> u32 {
    words[HEADER_WORDS + 3]
}

#[test]
fn a_blob_must_be_whole_words_with_the_magic_and_a_header() {
    assert_eq!(words(&[0, 1, 2]), Err(SpirvError::NotWords));
    assert_eq!(words(&bytes(&HEADER[..4])), Err(SpirvError::TooShort));
    let mut other = HEADER;
    other[0] = 0x0302_2307;
    assert_eq!(words(&bytes(&other)), Err(SpirvError::BadMagic));
    assert_eq!(words(&bytes(&HEADER)), Ok(HEADER.to_vec()));
}

#[test]
fn a_uniform_block_goes_to_the_uniform_set_and_keeps_its_binding() {
    let mut module = module(7, CLASS_UNIFORM, 0, 3);
    assert_eq!(retarget(&mut module), Ok(()));
    assert_eq!(set_of(&module), SET_UNIFORMS);
    assert_eq!(module[HEADER_WORDS + 4 + 3], 3);
}

#[test]
fn a_sampled_texture_goes_to_the_texture_set() {
    let mut module = module(7, CLASS_UNIFORM_CONSTANT, 0, 0);
    assert_eq!(retarget(&mut module), Ok(()));
    assert_eq!(set_of(&module), SET_TEXTURES);
}

#[test]
fn a_set_that_was_wrong_in_the_source_is_corrected() {
    let mut module = module(7, CLASS_UNIFORM_CONSTANT, 5, 1);
    assert_eq!(retarget(&mut module), Ok(()));
    assert_eq!(set_of(&module), SET_TEXTURES);
}

#[test]
fn a_decoration_of_something_else_is_left_alone() {
    // A location decoration (30) on an output variable.
    let mut words = HEADER.to_vec();
    words.extend(instruction(OP_DECORATE, &[7, 30, 1]));
    words.extend(instruction(OP_VARIABLE, &[9, 7, 3]));
    let before = words.clone();
    assert_eq!(retarget(&mut words), Ok(()));
    assert_eq!(words, before);
}

#[test]
fn a_storage_buffer_and_a_binding_beyond_the_slots_are_refused() {
    let mut storage = module(7, CLASS_STORAGE_BUFFER, 0, 0);
    assert_eq!(retarget(&mut storage), Err(SpirvError::UnsupportedResource));
    let mut high = module(7, CLASS_UNIFORM, 0, BINDINGS);
    assert_eq!(retarget(&mut high), Err(SpirvError::UnsupportedResource));
}

#[test]
fn an_instruction_that_runs_past_the_end_or_has_no_words_is_malformed() {
    let mut long = HEADER.to_vec();
    long.push(9 << 16 | OP_DECORATE);
    assert_eq!(retarget(&mut long), Err(SpirvError::Malformed));
    let mut empty = HEADER.to_vec();
    empty.push(OP_DECORATE);
    assert_eq!(retarget(&mut empty), Err(SpirvError::Malformed));
}

#[test]
fn a_module_without_resources_is_unchanged() {
    let mut words = HEADER.to_vec();
    let before = words.clone();
    assert_eq!(retarget(&mut words), Ok(()));
    assert_eq!(words, before);
}

#[test]
fn many_decorations_are_retargeted_in_one_walk() {
    // 20000 variables with 20000 decorations: a per-decoration walk would take 4e8 steps.
    let mut words = vec![0x0723_0203_u32, 0x0001_0600, 0, 20_001, 0];
    for id in 1..=20_000_u32 {
        words.extend([0x0004_003B, 1, id, 2]);
    }
    for id in 1..=20_000_u32 {
        words.extend([0x0004_0047, id, 34, 7]);
    }
    let started = std::time::Instant::now();
    assert_eq!(retarget(&mut words), Ok(()));
    assert!(started.elapsed().as_secs() < 5);
    assert_eq!(words[words.len() - 1], 0);
}
