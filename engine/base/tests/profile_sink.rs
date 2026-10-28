//! Scopes deliver enter and exit events, in order, to the one installed sink.
//! The sink is global and set once per process, so every test here shares `RECORDER`
//! and reads only the events of its own thread.

use base::profile::{ProfileSink, SinkAlreadyInstalled, install_sink, scope, sink_installed};
use std::sync::{Mutex, Once};
use std::thread::{self, ThreadId};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
    Enter,
    Exit,
}

struct Recorder {
    events: Mutex<Vec<(ThreadId, Kind, &'static str)>>,
}

impl Recorder {
    fn of_this_thread(&self) -> Vec<(Kind, &'static str)> {
        let me = thread::current().id();
        self.events
            .lock()
            .map(|events| {
                events
                    .iter()
                    .filter(|e| e.0 == me)
                    .map(|e| (e.1, e.2))
                    .collect()
            })
            .unwrap_or_default()
    }

    fn push(&self, kind: Kind, name: &'static str) {
        if let Ok(mut events) = self.events.lock() {
            events.push((thread::current().id(), kind, name));
        }
    }
}

impl ProfileSink for Recorder {
    fn enter(&self, name: &'static str) {
        self.push(Kind::Enter, name);
    }

    fn exit(&self, name: &'static str) {
        self.push(Kind::Exit, name);
    }
}

/// Runs `body` on a fresh thread, so its events are told apart from other tests', and fails if it panicked.
fn on_new_thread(body: fn()) {
    assert!(thread::spawn(body).join().is_ok());
}

static RECORDER: Recorder = Recorder {
    events: Mutex::new(Vec::new()),
};
static INSTALL: Once = Once::new();

fn install() {
    INSTALL.call_once(|| assert!(install_sink(&RECORDER).is_ok()));
}

#[test]
fn a_second_sink_is_refused_and_the_first_stays() {
    install();
    assert!(sink_installed());
    assert_eq!(install_sink(&RECORDER), Err(SinkAlreadyInstalled));
    let _guard = scope("still the first sink");
    assert_eq!(
        RECORDER.of_this_thread().last(),
        Some(&(Kind::Enter, "still the first sink"))
    );
}

#[test]
fn a_scope_delivers_enter_then_exit_with_its_name() {
    install();
    on_new_thread(|| {
        drop(scope("frame"));
        assert_eq!(
            RECORDER.of_this_thread(),
            [(Kind::Enter, "frame"), (Kind::Exit, "frame")]
        );
    });
}

#[test]
fn nested_scopes_exit_in_reverse_order() {
    install();
    on_new_thread(|| {
        {
            let _outer = scope("outer");
            let _inner = scope("inner");
        }
        let expected = [
            (Kind::Enter, "outer"),
            (Kind::Enter, "inner"),
            (Kind::Exit, "inner"),
            (Kind::Exit, "outer"),
        ];
        assert_eq!(RECORDER.of_this_thread(), expected);
    });
}

#[test]
fn a_scope_that_started_keeps_its_exit_when_dropped_late() {
    install();
    on_new_thread(|| {
        let guard = scope("long");
        let events_during = RECORDER.of_this_thread();
        drop(guard);
        assert_eq!(events_during, [(Kind::Enter, "long")]);
        assert_eq!(
            RECORDER.of_this_thread().last(),
            Some(&(Kind::Exit, "long"))
        );
    });
}

#[cfg(feature = "profiling")]
#[test]
fn the_macro_opens_a_scope_that_closes_at_the_end_of_the_block() {
    install();
    on_new_thread(|| {
        {
            base::profile_scope!("macro scope");
            assert_eq!(RECORDER.of_this_thread(), [(Kind::Enter, "macro scope")]);
        }
        assert_eq!(
            RECORDER.of_this_thread(),
            [(Kind::Enter, "macro scope"), (Kind::Exit, "macro scope")]
        );
    });
}
