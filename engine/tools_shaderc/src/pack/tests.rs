use super::*;
use crate::compiler::{CompiledSpirv, Reflection};
use assets::dshd::{Program, Stage, UniformBlock, VertexInput};

fn vertex() -> CompiledModule {
    CompiledModule {
        stage: Stage::Vertex,
        code: "void main() {}\n".to_string(),
        reflection: Reflection {
            uniform: Some(UniformBlock {
                binding: 0,
                size: 64,
            }),
            texture_binding: None,
            inputs: vec![VertexInput {
                location: 0,
                components: 2,
            }],
        },
    }
}

fn fragment() -> CompiledModule {
    CompiledModule {
        stage: Stage::Fragment,
        code: "void main() { }\n".to_string(),
        reflection: Reflection {
            uniform: None,
            texture_binding: Some(0),
            inputs: Vec::new(),
        },
    }
}

fn spirv(stage: Stage, reflection: Reflection) -> CompiledSpirv {
    let code = [0x0723_0203u32, 0x0001_0000, 0, 1, 0]
        .iter()
        .flat_map(|w| w.to_le_bytes())
        .collect();
    CompiledSpirv {
        stage,
        code,
        reflection,
    }
}

#[test]
fn packed_spirv_reads_back_with_the_blobs_and_the_reflection() {
    let (v, f) = (vertex(), fragment());
    let modules = [
        spirv(Stage::Vertex, v.reflection.clone()),
        spirv(Stage::Fragment, f.reflection.clone()),
    ];
    let bytes = pack_spirv(&modules).unwrap_or_default();
    let program = Program::parse(&bytes);
    assert!(program.is_ok(), "{program:?}");
    let program = program.unwrap_or_else(|_| unreachable!());
    assert_eq!(program.backend(), assets::dshd::Backend::SpirV);
    assert!(program.module_for(Stage::Vertex).is_some_and(|m| {
        m.spirv() == Some(modules[0].code.as_slice())
            && m.uniform() == v.reflection.uniform
            && m.input_count() == 1
    }));
    assert!(
        program
            .module_for(Stage::Fragment)
            .is_some_and(|m| m.texture_binding() == Some(0))
    );
}

#[test]
fn spirv_that_is_not_a_binary_is_not_packed() {
    let mut bad = spirv(Stage::Vertex, Reflection::default());
    bad.code[0] ^= 1;
    assert_eq!(pack_spirv(&[bad]), Err(DshdError::BadSpirv));
    assert_eq!(pack_spirv(&[]), Err(DshdError::BadModuleCount));
}

#[test]
fn packed_bytes_read_back_as_the_same_program() {
    let bytes = pack(&[vertex(), fragment()]).unwrap_or_default();
    let program = Program::parse(&bytes);
    assert!(program.is_ok());
    let program = program.unwrap_or_else(|_| unreachable!());
    assert_eq!(program.module_count(), 2);
    let v = program.module_for(Stage::Vertex);
    assert!(v.is_some_and(|m| m.code() == "void main() {}\n"
        && m.uniform()
            == Some(UniformBlock {
                binding: 0,
                size: 64
            })
        && m.input_count() == 1));
    let f = program.module_for(Stage::Fragment);
    assert!(f.is_some_and(|m| m.texture_binding() == Some(0) && m.code() == "void main() { }\n"));
}

#[test]
fn packing_is_deterministic_and_has_no_slack() {
    let a = pack(&[vertex(), fragment()]).unwrap_or_default();
    let b = pack(&[vertex(), fragment()]).unwrap_or_default();
    assert!(!a.is_empty());
    assert_eq!(a, b);
    assert!(Program::parse(&a).is_ok());
}

#[test]
fn bad_module_sets_are_refused() {
    assert!(pack(&[]).is_err());
    assert!(pack(&[vertex(), vertex()]).is_err());
    let mut empty = vertex();
    empty.code.clear();
    assert!(pack(&[empty]).is_err());
}
