//! A scope opened before a sink exists never reports an exit to a sink installed later:
//! the guard keeps the sink it entered, so an exit always pairs with its enter.

use base::profile::{ProfileSink, install_sink, scope};
use std::sync::Mutex;

struct Recorder {
    events: Mutex<Vec<(bool, &'static str)>>,
}

impl Recorder {
    fn events(&self) -> Vec<(bool, &'static str)> {
        self.events
            .lock()
            .map(|events| events.clone())
            .unwrap_or_default()
    }

    fn push(&self, entered: bool, name: &'static str) {
        if let Ok(mut events) = self.events.lock() {
            events.push((entered, name));
        }
    }
}

impl ProfileSink for Recorder {
    fn enter(&self, name: &'static str) {
        self.push(true, name);
    }

    fn exit(&self, name: &'static str) {
        self.push(false, name);
    }
}

static RECORDER: Recorder = Recorder {
    events: Mutex::new(Vec::new()),
};

#[test]
fn a_scope_opened_before_the_sink_does_not_report_an_exit_to_it() {
    let early = scope("early");
    assert!(install_sink(&RECORDER).is_ok());
    drop(early);
    assert_eq!(RECORDER.events(), []);
    drop(scope("late"));
    assert_eq!(RECORDER.events(), [(true, "late"), (false, "late")]);
}
