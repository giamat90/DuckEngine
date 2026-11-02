use super::*;
use crate::compiler::Reflection;
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
