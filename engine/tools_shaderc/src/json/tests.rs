use super::*;

fn must<T, E>(r: Result<T, E>) -> T {
    r.unwrap_or_else(|_| unreachable!())
}

#[test]
fn scalars() {
    assert_eq!(must(parse("null")), Json::Null);
    assert_eq!(must(parse("true")), Json::Bool(true));
    assert_eq!(must(parse(" false ")), Json::Bool(false));
    assert_eq!(must(parse("\"hi\"")), Json::String("hi".to_string()));
    assert_eq!(must(parse("42")).as_u64(), Some(42));
    assert_eq!(must(parse("0")).as_u64(), Some(0));
}

#[test]
fn numbers_that_are_not_unsigned_integers() {
    for text in ["-1", "1.5", "1e3", "-0", "2.0"] {
        let value = must(parse(text));
        assert!(matches!(value, Json::Number(_)), "{text}");
        assert_eq!(value.as_u64(), None, "{text}");
    }
    assert_eq!(Json::Null.as_u64(), None);
    assert_eq!(Json::String("1".to_string()).as_u64(), None);
}

#[test]
fn malformed_numbers_are_refused() {
    for text in ["01", "-", "1.", ".5", "1e", "+1", "1e+", "--1"] {
        assert!(parse(text).is_err(), "{text}");
    }
}

#[test]
fn strings_with_escapes() {
    let value = must(parse(r#""a\"b\\c\/d\n\tAé""#));
    assert_eq!(value.as_str(), Some("a\"b\\c/d\n\tA\u{e9}"));
    assert!(parse(r#""\x""#).is_err());
    assert!(parse(r#""\u12""#).is_err());
    assert!(parse(r#""\ud800""#).is_err());
    assert!(parse("\"open").is_err());
    assert!(parse("\"tab\there\"").is_err());
}

#[test]
fn every_escape_and_text_after_it() {
    let backslash_u = "\\u";
    let text = format!("\"\\b\\f\\r{backslash_u}0041b{backslash_u}00e9z\"");
    let value = must(parse(&text));
    assert_eq!(value.as_str(), Some("\u{8}\u{c}\rAb\u{e9}z"));
    assert!(parse(r#""\u+123""#).is_err());
    assert!(parse(r#""\u12g4""#).is_err());
}

#[test]
fn numbers_with_exponents_inside_containers() {
    let value = must(parse("[1e5, 2E+3, 4e-2, 7]"));
    let items = value.as_array().unwrap_or(&[]);
    assert_eq!(items.len(), 4);
    assert_eq!(items[3].as_u64(), Some(7));
    assert_eq!(items[0].as_u64(), None);
}

#[test]
fn mismatched_closers_are_refused() {
    assert!(parse("[1}").is_err());
    assert!(parse("{\"a\":1]").is_err());
}

#[test]
fn input_of_exactly_the_limit_is_accepted() {
    let text = format!("\"{}\"", "a".repeat(MAX_INPUT_LEN - 2));
    assert_eq!(text.len(), MAX_INPUT_LEN);
    assert!(parse(&text).is_ok());
}

#[test]
fn arrays_and_objects() {
    let value = must(parse(r#"{"a": [1, 2, {"b": null}], "c": "d", "e": []}"#));
    assert_eq!(value.get("c").and_then(Json::as_str), Some("d"));
    let a = value.get("a").and_then(Json::as_array).unwrap_or(&[]);
    assert_eq!(a.len(), 3);
    assert_eq!(a[1].as_u64(), Some(2));
    assert_eq!(a[2].get("b"), Some(&Json::Null));
    assert_eq!(value.get("e").and_then(Json::as_array), Some(&[][..]));
    assert_eq!(value.get("missing"), None);
    assert_eq!(Json::Null.get("a"), None);
    assert_eq!(Json::Null.as_array(), None);
    assert_eq!(Json::Null.as_str(), None);
}

#[test]
fn the_first_of_two_equal_keys_wins() {
    let value = must(parse(r#"{"k": 1, "k": 2}"#));
    assert_eq!(value.get("k").and_then(Json::as_u64), Some(1));
}

#[test]
fn syntax_errors() {
    for text in [
        "",
        " ",
        "nul",
        "tru",
        "[1,]",
        "[1 2]",
        "{\"a\"}",
        "{\"a\":}",
        "{a:1}",
        "{\"a\":1,}",
        "[",
        "{",
        "1 2",
        "[] x",
    ] {
        assert!(parse(text).is_err(), "{text:?}");
    }
}

#[test]
fn nesting_has_a_depth_limit() {
    let deep = |n: usize| format!("{}{}", "[".repeat(n), "]".repeat(n));
    assert!(parse(&deep(MAX_DEPTH)).is_ok());
    assert_eq!(parse(&deep(MAX_DEPTH + 1)), Err(JsonError::TooDeep));
}

#[test]
fn input_size_has_a_limit() {
    let big = format!("\"{}\"", "a".repeat(MAX_INPUT_LEN));
    assert_eq!(parse(&big), Err(JsonError::TooLarge));
}

#[test]
fn every_error_has_its_own_message() {
    let all = [
        JsonError::Syntax,
        JsonError::TooDeep,
        JsonError::TooLarge,
        JsonError::BadEscape,
    ];
    let texts: Vec<String> = all.iter().map(ToString::to_string).collect();
    for (i, a) in texts.iter().enumerate() {
        assert!(!a.is_empty());
        assert!(texts[i + 1..].iter().all(|b| a != b));
    }
}

#[test]
fn the_reflection_output_of_spirv_cross_parses() {
    let text = r#"{
    "entryPoints" : [ { "name" : "main", "mode" : "vert" } ],
    "inputs" : [ { "type" : "vec2", "name" : "in_pos", "location" : 0 } ],
    "ubos" : [ { "type" : "_17", "name" : "Camera", "block_size" : 64, "set" : 0, "binding" : 0 } ]
}"#;
    let value = must(parse(text));
    let ubo = &value.get("ubos").and_then(Json::as_array).unwrap_or(&[])[0];
    assert_eq!(ubo.get("block_size").and_then(Json::as_u64), Some(64));
}
