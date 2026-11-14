//! Components registered at run time need no `unsafe` in the safe column: the constructor of a
//! column is a plain function pointer captured where the type is still known.

use tools_ecs_spike::column::{ColumnOps, SafeColumn};

/// What registering a component type stores: how to make its (empty) column.
type Constructor = fn() -> SafeColumn;

fn register<T: 'static>(registry: &mut Vec<Constructor>) -> usize {
    registry.push(SafeColumn::new::<T>);
    registry.len() - 1
}

#[test]
fn columns_are_built_from_registered_constructors_and_rows_move_between_them() {
    let mut registry: Vec<Constructor> = Vec::new();
    let position = register::<(f32, f32)>(&mut registry);
    let name = register::<String>(&mut registry);

    let mut from: Vec<SafeColumn> = registry.iter().map(|make| make()).collect();
    let mut to: Vec<SafeColumn> = registry.iter().map(|make| make()).collect();
    from[position].push((1.0_f32, 2.0_f32));
    from[position].push((3.0_f32, 4.0_f32));
    from[name].push("first".to_string());
    from[name].push("second".to_string());

    for id in [position, name] {
        let (src, dst) = (&mut from[id], &mut to[id]);
        src.swap_remove_into(0, dst);
    }
    assert_eq!(to[position].as_slice::<(f32, f32)>(), [(1.0, 2.0)]);
    assert_eq!(to[name].as_slice::<String>(), ["first".to_string()]);
    assert_eq!(from[position].as_slice::<(f32, f32)>(), [(3.0, 4.0)]);
    assert_eq!(from[name].as_slice::<String>(), ["second".to_string()]);
}
