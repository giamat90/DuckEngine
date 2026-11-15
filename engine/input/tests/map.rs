//! The input map: named actions and contexts, and bindings that must fit the kind of the action.

use input::{ActionKind, Binding, InputError, InputMap, KeyCode};

#[test]
fn actions_and_contexts_are_found_by_name() -> Result<(), InputError> {
    let mut map = InputMap::new();
    let jump = map.add_action("jump", ActionKind::Button)?;
    let walk = map.add_action("walk", ActionKind::Axis)?;
    let play = map.add_context("gameplay")?;
    assert_eq!(map.action("jump"), Some(jump));
    assert_eq!(map.action("walk"), Some(walk));
    assert_eq!(map.action("missing"), None);
    assert_eq!(map.context("gameplay"), Some(play));
    assert_eq!(map.context("menu"), None);
    assert_eq!(map.action_name(walk), Some("walk"));
    assert_eq!(map.action_kind(jump), Some(ActionKind::Button));
    assert_eq!(map.action_kind(walk), Some(ActionKind::Axis));
    assert_eq!((map.action_count(), map.context_count()), (2, 1));
    Ok(())
}

#[test]
fn a_name_can_be_used_once_per_kind_of_thing() -> Result<(), InputError> {
    let mut map = InputMap::new();
    map.add_action("fire", ActionKind::Button)?;
    map.add_context("fire")?;
    assert_eq!(
        map.add_action("fire", ActionKind::Axis),
        Err(InputError::DuplicateName)
    );
    assert_eq!(map.add_context("fire"), Err(InputError::DuplicateName));
    assert_eq!(map.action_count(), 1);
    Ok(())
}

#[test]
fn a_button_takes_keys_and_mouse_buttons_and_an_axis_takes_pairs_and_the_wheel()
-> Result<(), InputError> {
    let mut map = InputMap::new();
    let fire = map.add_action("fire", ActionKind::Button)?;
    let walk = map.add_action("walk", ActionKind::Axis)?;
    let ctx = map.add_context("gameplay")?;
    map.bind(ctx, fire, Binding::Key(KeyCode::Space))?;
    map.bind(ctx, fire, Binding::MouseButton(0))?;
    map.bind(ctx, walk, Binding::scroll_y())?;
    map.bind(ctx, walk, Binding::keys(KeyCode::A, KeyCode::D))?;
    assert_eq!(
        map.bind(ctx, fire, Binding::scroll_y()),
        Err(InputError::WrongKind)
    );
    assert_eq!(
        map.bind(ctx, fire, Binding::keys(KeyCode::A, KeyCode::D)),
        Err(InputError::WrongKind)
    );
    assert_eq!(
        map.bind(ctx, walk, Binding::Key(KeyCode::W)),
        Err(InputError::WrongKind)
    );
    assert_eq!(
        map.bind(ctx, walk, Binding::MouseButton(1)),
        Err(InputError::WrongKind)
    );
    Ok(())
}

#[test]
fn ids_beyond_this_map_are_refused() -> Result<(), InputError> {
    let mut bigger = InputMap::new();
    bigger.add_action("first", ActionKind::Button)?;
    let far_action = bigger.add_action("second", ActionKind::Button)?;
    bigger.add_context("first")?;
    let far_context = bigger.add_context("second")?;
    let mut map = InputMap::new();
    let action = map.add_action("x", ActionKind::Button)?;
    let context = map.add_context("y")?;
    assert_eq!(
        map.bind(context, far_action, Binding::Key(KeyCode::A)),
        Err(InputError::UnknownAction)
    );
    assert_eq!(
        map.bind(far_context, action, Binding::Key(KeyCode::A)),
        Err(InputError::UnknownContext)
    );
    assert_eq!(map.action_name(far_action), None);
    assert_eq!(map.action_kind(far_action), None);
    Ok(())
}

#[test]
fn every_error_has_its_own_message() {
    let all = [
        InputError::DuplicateName,
        InputError::UnknownAction,
        InputError::UnknownContext,
        InputError::WrongKind,
        InputError::BadButton,
        InputError::TooMany,
    ];
    let texts: Vec<String> = all.iter().map(ToString::to_string).collect();
    for (at, text) in texts.iter().enumerate() {
        assert!(!text.is_empty());
        assert_eq!(
            texts.iter().filter(|other| *other == text).count(),
            1,
            "{at}"
        );
    }
}

#[test]
fn a_mouse_button_past_the_last_one_is_refused() -> Result<(), InputError> {
    let mut map = InputMap::new();
    let fire = map.add_action("fire", ActionKind::Button)?;
    let ctx = map.add_context("c")?;
    map.bind(ctx, fire, Binding::MouseButton(7))?;
    assert_eq!(
        map.bind(ctx, fire, Binding::MouseButton(8)),
        Err(InputError::BadButton)
    );
    Ok(())
}

#[test]
fn more_actions_than_an_id_can_name_are_refused() -> Result<(), InputError> {
    let mut map = InputMap::new();
    for n in 0..=u32::from(u16::MAX) {
        map.add_action(&n.to_string(), ActionKind::Button)?;
    }
    assert_eq!(
        map.add_action("one more", ActionKind::Button),
        Err(InputError::TooMany)
    );
    Ok(())
}
