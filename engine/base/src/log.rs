//! Leveled logging through a sink the caller owns. No global state and no OS
//! access: a platform adapter supplies the sink that reaches files or a console.

use core::fmt;

/// Severity, most severe first, so `Error < Warn < Info < Debug < Trace`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Level {
    Error,
    Warn,
    Info,
    Debug,
    Trace,
}

/// One log event. The message is not formatted until a sink asks for it.
#[derive(Debug, Clone, Copy)]
pub struct Record<'a> {
    pub level: Level,
    pub target: &'a str,
    pub message: fmt::Arguments<'a>,
}

/// Destination of log records.
pub trait LogSink {
    /// Handles one record. A sink has a single owner; sharing across threads is not supported yet.
    fn write(&mut self, record: &Record<'_>);
}

/// Discards everything without formatting it.
#[derive(Debug, Default, Clone, Copy)]
pub struct NullSink;

impl LogSink for NullSink {
    fn write(&mut self, _record: &Record<'_>) {}
}

/// One record kept by [`MemorySink`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    pub level: Level,
    pub target: String,
    pub message: String,
}

/// Keeps every record in memory; the test double for code that logs.
#[derive(Debug, Default)]
pub struct MemorySink {
    entries: Vec<Entry>,
}

impl MemorySink {
    /// An empty sink.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    #[must_use]
    pub fn entries(&self) -> &[Entry] {
        &self.entries
    }
}

impl LogSink for MemorySink {
    fn write(&mut self, record: &Record<'_>) {
        self.entries.push(Entry {
            level: record.level,
            target: record.target.to_owned(),
            message: record.message.to_string(),
        });
    }
}

/// Filters by level and forwards to a sink.
#[derive(Debug)]
pub struct Logger<S: LogSink> {
    sink: S,
    max_level: Level,
}

impl<S: LogSink> Logger<S> {
    /// Records more verbose than `max_level` are dropped.
    #[must_use]
    pub fn new(sink: S, max_level: Level) -> Self {
        Self { sink, max_level }
    }

    #[must_use]
    pub fn enabled(&self, level: Level) -> bool {
        level <= self.max_level
    }

    /// Forwards the record when `level` passes the limit. The message is not formatted
    /// here, but the caller's `format_args!` arguments are evaluated either way.
    pub fn log(&mut self, level: Level, target: &str, message: fmt::Arguments<'_>) {
        if self.enabled(level) {
            self.sink.write(&Record {
                level,
                target,
                message,
            });
        }
    }

    #[must_use]
    pub fn sink(&self) -> &S {
        &self.sink
    }

    #[must_use]
    pub fn into_sink(self) -> S {
        self.sink
    }
}

#[cfg(test)]
mod tests {
    use super::{Entry, Level, LogSink, Logger, MemorySink, NullSink, Record};
    use core::cell::Cell;
    use core::fmt;

    fn logger(max: Level) -> Logger<MemorySink> {
        Logger::new(MemorySink::new(), max)
    }

    #[test]
    fn levels_order_from_most_to_least_severe() {
        assert!(Level::Error < Level::Warn);
        assert!(Level::Warn < Level::Info);
        assert!(Level::Info < Level::Debug);
        assert!(Level::Debug < Level::Trace);
    }

    #[test]
    fn record_is_delivered_with_level_target_and_message() {
        let mut log = logger(Level::Trace);
        log.log(Level::Info, "audio", format_args!("rate {}", 48_000));
        assert_eq!(
            log.sink().entries(),
            [Entry {
                level: Level::Info,
                target: "audio".to_owned(),
                message: "rate 48000".to_owned()
            }]
        );
    }

    #[test]
    fn records_are_kept_in_call_order() {
        let mut log = logger(Level::Trace);
        log.log(Level::Warn, "a", format_args!("first"));
        log.log(Level::Error, "b", format_args!("second"));
        let messages: Vec<&str> = log
            .sink()
            .entries()
            .iter()
            .map(|e| e.message.as_str())
            .collect();
        assert_eq!(messages, ["first", "second"]);
    }

    #[test]
    fn records_more_verbose_than_the_limit_are_dropped() {
        let mut log = logger(Level::Warn);
        log.log(Level::Info, "t", format_args!("dropped"));
        log.log(Level::Warn, "t", format_args!("kept at the limit"));
        log.log(Level::Error, "t", format_args!("kept"));
        assert_eq!(log.sink().entries().len(), 2);
    }

    #[test]
    fn enabled_matches_the_limit() {
        let log = logger(Level::Info);
        assert!(log.enabled(Level::Error));
        assert!(log.enabled(Level::Info));
        assert!(!log.enabled(Level::Debug));
    }

    struct LastLevel(Option<Level>);

    impl LogSink for LastLevel {
        fn write(&mut self, record: &Record<'_>) {
            self.0 = Some(record.level);
        }
    }

    struct CountsFormatting<'a>(&'a Cell<u32>);

    impl fmt::Display for CountsFormatting<'_> {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            self.0.set(self.0.get() + 1);
            f.write_str("x")
        }
    }

    #[test]
    fn null_sink_and_filtered_records_never_format_the_message() {
        let formatted = Cell::new(0);
        let mut counted = logger(Level::Trace);
        counted.log(
            Level::Info,
            "t",
            format_args!("{}", CountsFormatting(&formatted)),
        );
        assert_eq!(formatted.get(), 1, "a memory sink formats the message once");
        formatted.set(0);
        let mut quiet = Logger::new(NullSink, Level::Trace);
        quiet.log(
            Level::Info,
            "t",
            format_args!("{}", CountsFormatting(&formatted)),
        );
        let mut filtered = logger(Level::Error);
        filtered.log(
            Level::Info,
            "t",
            format_args!("{}", CountsFormatting(&formatted)),
        );
        assert_eq!(formatted.get(), 0);
    }

    #[test]
    fn a_custom_sink_receives_the_record() {
        let mut log = Logger::new(LastLevel(None), Level::Trace);
        log.log(Level::Debug, "t", format_args!("m"));
        assert_eq!(log.into_sink().0, Some(Level::Debug));
    }
}
