use super::*;
use assets::dshd::{UniformBlock, VertexInput};

fn must<T, E>(r: Result<T, E>) -> T {
    r.unwrap_or_else(|_| unreachable!())
}

const VERTEX: &str = r#"{
  "entryPoints": [{"name": "main", "mode": "vert"}],
  "inputs": [
    {"type": "vec2", "name": "in_pos", "location": 0},
    {"type": "vec2", "name": "in_uv", "location": 1},
    {"type": "vec4", "name": "in_color", "location": 2}
  ],
  "outputs": [{"type": "vec2", "name": "v_uv", "location": 0}],
  "ubos": [{"type": "_17", "name": "Camera", "block_size": 64, "set": 0, "binding": 0}]
}"#;

const FRAGMENT: &str = r#"{
  "entryPoints": [{"name": "main", "mode": "frag"}],
  "inputs": [
    {"type": "vec2", "name": "v_uv", "location": 0},
    {"type": "vec4", "name": "v_color", "location": 1}
  ],
  "outputs": [{"type": "vec4", "name": "out_color", "location": 0}],
  "textures": [{"type": "sampler2D", "name": "tex", "set": 0, "binding": 3}]
}"#;

#[test]
fn a_resource_key_of_the_wrong_type_is_malformed_not_empty() {
    for key in ["inputs", "ubos", "textures", "ssbos", "push_constants"] {
        for wrong in ["{}", "\"x\"", "7", "null", "true"] {
            let json = format!("{{\"{key}\": {wrong}}}");
            let result = parse(&json, Stage::Vertex);
            assert!(
                matches!(result, Err(ReflectError::Malformed(_))),
                "{key}: {wrong}: {result:?}"
            );
        }
    }
}

#[test]
fn an_absent_resource_key_means_none_because_spirv_cross_omits_empty_lists() {
    let r = must(parse("{}", Stage::Vertex));
    assert_eq!(r, Reflection::default());
    assert_eq!(must(parse("{}", Stage::Fragment)), Reflection::default());
}

#[test]
fn sixteen_inputs_are_accepted_and_seventeen_are_not() {
    let list = |n: usize| {
        let items: Vec<String> = (0..n)
            .map(|i| format!("{{\"type\": \"vec4\", \"name\": \"i\", \"location\": {i}}}"))
            .collect();
        format!("{{\"inputs\": [{}]}}", items.join(","))
    };
    assert_eq!(must(parse(&list(16), Stage::Vertex)).inputs.len(), 16);
    assert!(matches!(
        parse(&list(17), Stage::Vertex),
        Err(ReflectError::Unsupported(_))
    ));
}

#[test]
fn array_resources_are_refused() {
    let ubo =
        r#"{"ubos": [{"name": "A", "block_size": 64, "set": 0, "binding": 0, "array": [2]}]}"#;
    let tex = r#"{"textures": [{"type": "sampler2D", "name": "t", "set": 0, "binding": 0, "array": [4]}]}"#;
    let input = r#"{"inputs": [{"type": "vec2", "name": "p", "location": 0, "array": [2]}]}"#;
    assert!(matches!(
        parse(ubo, Stage::Vertex),
        Err(ReflectError::Unsupported(_))
    ));
    assert!(matches!(
        parse(tex, Stage::Fragment),
        Err(ReflectError::Unsupported(_))
    ));
    assert!(matches!(
        parse(input, Stage::Vertex),
        Err(ReflectError::Unsupported(_))
    ));
}

#[test]
fn vertex_reflection() {
    let r = must(parse(VERTEX, Stage::Vertex));
    assert_eq!(
        r.uniform,
        Some(UniformBlock {
            binding: 0,
            size: 64
        })
    );
    assert_eq!(r.texture_binding, None);
    assert_eq!(
        r.inputs,
        vec![
            VertexInput {
                location: 0,
                components: 2
            },
            VertexInput {
                location: 1,
                components: 2
            },
            VertexInput {
                location: 2,
                components: 4
            },
        ]
    );
}

#[test]
fn fragment_reflection_ignores_varyings() {
    let r = must(parse(FRAGMENT, Stage::Fragment));
    assert_eq!(r.uniform, None);
    assert_eq!(r.texture_binding, Some(3));
    assert!(r.inputs.is_empty());
}

#[test]
fn inputs_are_sorted_by_location() {
    let text = r#"{"inputs": [
        {"type": "vec4", "location": 5}, {"type": "float", "location": 1}, {"type": "vec3", "location": 2}]}"#;
    let r = must(parse(text, Stage::Vertex));
    let locations: Vec<u8> = r.inputs.iter().map(|i| i.location).collect();
    assert_eq!(locations, vec![1, 2, 5]);
    let widths: Vec<u8> = r.inputs.iter().map(|i| i.components).collect();
    assert_eq!(widths, vec![1, 3, 4]);
}

#[test]
fn a_module_without_resources_reflects_to_nothing() {
    let r = must(parse("{}", Stage::Fragment));
    assert_eq!(r, Reflection::default());
}

#[test]
fn unsupported_things_are_refused_with_a_reason() {
    let cases = [
        (
            r#"{"inputs": [{"type": "ivec2", "location": 0}]}"#,
            Stage::Vertex,
        ),
        (
            r#"{"inputs": [{"type": "mat4", "location": 0}]}"#,
            Stage::Vertex,
        ),
        (r#"{"inputs": [{"type": "vec2"}]}"#, Stage::Vertex),
        (r#"{"inputs": [{"location": 0}]}"#, Stage::Vertex),
        (
            r#"{"inputs": [{"type": "vec2", "location": 256}]}"#,
            Stage::Vertex,
        ),
        (
            r#"{"ubos": [{"block_size": 4, "set": 0, "binding": 0}, {"block_size": 4, "set": 0, "binding": 1}]}"#,
            Stage::Vertex,
        ),
        (
            r#"{"ubos": [{"block_size": 4, "set": 1, "binding": 0}]}"#,
            Stage::Vertex,
        ),
        (
            r#"{"ubos": [{"block_size": 4, "set": 0, "binding": 255}]}"#,
            Stage::Vertex,
        ),
        (
            r#"{"ubos": [{"block_size": 4, "set": 0, "binding": 300}]}"#,
            Stage::Vertex,
        ),
        (
            r#"{"ubos": [{"block_size": 0, "set": 0, "binding": 0}]}"#,
            Stage::Vertex,
        ),
        (r#"{"ubos": [{"set": 0, "binding": 0}]}"#, Stage::Vertex),
        (r#"{"ubos": [{"block_size": 4, "set": 0}]}"#, Stage::Vertex),
        (
            r#"{"textures": [{"binding": 0}, {"binding": 1}]}"#,
            Stage::Fragment,
        ),
        (r#"{"textures": [{"set": 0}]}"#, Stage::Fragment),
        (
            r#"{"textures": [{"set": 2, "binding": 0}]}"#,
            Stage::Fragment,
        ),
        (r#"{"separate_images": [{"binding": 0}]}"#, Stage::Fragment),
        (
            r#"{"separate_samplers": [{"binding": 1}]}"#,
            Stage::Fragment,
        ),
        (r#"{"ssbos": [{"binding": 1}]}"#, Stage::Fragment),
        (r#"{"push_constants": [{"name": "p"}]}"#, Stage::Vertex),
        (r#"{"images": [{"binding": 1}]}"#, Stage::Fragment),
        (r#"{"subpass_inputs": [{"binding": 1}]}"#, Stage::Fragment),
        (r#"{"atomic_counters": [{"binding": 1}]}"#, Stage::Fragment),
    ];
    for (text, stage) in cases {
        assert!(parse(text, stage).is_err(), "{text}");
    }
}

#[test]
fn separate_samplers_get_a_helpful_message() {
    let err = parse(
        FRAGMENT.replace("textures", "separate_images").as_str(),
        Stage::Fragment,
    );
    let text = err.map_or_else(|e| e.to_string(), |_| String::new());
    assert!(text.contains("combined"), "{text}");
}

#[test]
fn too_many_inputs_and_bad_json_are_errors() {
    let many: Vec<String> = (0..17)
        .map(|i| format!(r#"{{"type": "vec2", "location": {i}}}"#))
        .collect();
    let text = format!(r#"{{"inputs": [{}]}}"#, many.join(","));
    assert!(parse(&text, Stage::Vertex).is_err());
    assert_eq!(
        parse("{", Stage::Vertex),
        Err(ReflectError::Json(JsonError::Syntax))
    );
    assert!(parse("[]", Stage::Vertex).is_err());
}

#[test]
fn every_error_has_its_own_message() {
    let all = [
        ReflectError::Json(JsonError::Syntax),
        ReflectError::Unsupported("x"),
        ReflectError::Malformed("x"),
    ];
    let texts: Vec<String> = all.iter().map(ToString::to_string).collect();
    for (i, a) in texts.iter().enumerate() {
        assert!(!a.is_empty());
        assert!(texts[i + 1..].iter().all(|b| a != b));
    }
}
