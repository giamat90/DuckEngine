//! The validation messages the Khronos validation layer sends, counted and kept.
//!
//! A [`ValidationLog`] is the destination of the debug messenger the instance creates when
//! validation is asked for. The log is thread-safe: the layer may call from any thread that calls
//! Vulkan. Nothing here panics or fails; a message is a value in the report.
#![expect(
    unsafe_code,
    reason = "the callback is called by the C layer with raw pointers; every unsafe block has a SAFETY comment"
)]

use crate::vk::{Bool32, DebugCallbackData, SEVERITY_ERROR, SEVERITY_WARNING};
use core::ffi::{CStr, c_void};
use std::sync::{Mutex, PoisonError};

/// How many message texts the log keeps; the counts go on rising after that.
pub const MAX_KEPT_MESSAGES: usize = 16;

/// What the validation layer reported so far.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ValidationReport {
    /// Messages of severity error.
    pub errors: u32,
    /// Messages of severity warning.
    pub warnings: u32,
    /// The first messages, in order, at most [`MAX_KEPT_MESSAGES`].
    pub messages: Vec<String>,
}

impl ValidationReport {
    /// No errors and no warnings.
    #[must_use]
    pub fn is_clean(&self) -> bool {
        self.errors == 0 && self.warnings == 0
    }
}

/// The shared destination of the validation callback.
#[derive(Default)]
pub struct ValidationLog(Mutex<ValidationReport>);

impl ValidationLog {
    /// Records one message of the given `VkDebugUtilsMessageSeverityFlagBitsEXT`. Information and
    /// verbose messages are not counted.
    pub fn record(&self, severity: u32, text: &str) {
        let mut report = self.0.lock().unwrap_or_else(PoisonError::into_inner);
        if severity & SEVERITY_ERROR != 0 {
            report.errors = report.errors.saturating_add(1);
        } else if severity & SEVERITY_WARNING != 0 {
            report.warnings = report.warnings.saturating_add(1);
        } else {
            return;
        }
        if report.messages.len() < MAX_KEPT_MESSAGES {
            report.messages.push(text.to_owned());
        }
    }

    /// A copy of what was recorded.
    #[must_use]
    pub fn report(&self) -> ValidationReport {
        self.0
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }
}

/// The `PFN_vkDebugUtilsMessengerCallbackEXT` the instance registers. Always returns
/// `VK_FALSE`: the call that caused the message goes on.
///
/// # Safety
/// `user` is null or points to a live [`ValidationLog`]; `data` is null or points to a callback
/// data record whose `message` is null or a NUL-terminated string, valid for the call. The
/// validation layer provides both.
pub unsafe extern "system" fn callback(
    severity: u32,
    _types: u32,
    data: *const DebugCallbackData,
    user: *mut c_void,
) -> Bool32 {
    if data.is_null() || user.is_null() {
        return 0;
    }
    // SAFETY: by the contract above, `user` points to the log the instance keeps alive for as
    // long as the messenger exists.
    let log = unsafe { &*user.cast::<ValidationLog>() };
    // SAFETY: by the contract above, `data` is valid for the call.
    let message = unsafe { (*data).message };
    let text = if message.is_null() {
        "(no message)".into()
    } else {
        // SAFETY: by the contract above, a non-null `message` is NUL-terminated and valid.
        unsafe { CStr::from_ptr(message) }.to_string_lossy()
    };
    log.record(severity, &text);
    0
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::vk::DebugCallbackData;
    use core::ptr::{null, null_mut};

    const INFO: u32 = 0x10;
    const VERBOSE: u32 = 0x1;

    fn data(message: *const core::ffi::c_char) -> DebugCallbackData {
        DebugCallbackData {
            s_type: 0,
            p_next: null(),
            flags: 0,
            message_id_name: null(),
            message_id_number: 0,
            message,
            queue_label_count: 0,
            queue_labels: null(),
            cmd_buf_label_count: 0,
            cmd_buf_labels: null(),
            object_count: 0,
            objects: null(),
        }
    }

    fn deliver(log: &ValidationLog, severity: u32, record: &DebugCallbackData) -> Bool32 {
        let user = core::ptr::from_ref(log).cast_mut().cast::<c_void>();
        // SAFETY: `user` is the live log; `record` is a valid record with a NUL-terminated message.
        unsafe { callback(severity, 2, record, user) }
    }

    #[test]
    fn a_new_log_is_clean() {
        let report = ValidationLog::default().report();
        assert!(report.is_clean());
        assert!(report.messages.is_empty());
    }

    #[test]
    fn errors_and_warnings_are_counted_and_kept_in_order() {
        let log = ValidationLog::default();
        assert_eq!(deliver(&log, SEVERITY_ERROR, &data(c"first".as_ptr())), 0);
        deliver(&log, SEVERITY_WARNING, &data(c"second".as_ptr()));
        deliver(&log, SEVERITY_ERROR, &data(c"third".as_ptr()));
        let report = log.report();
        assert_eq!((report.errors, report.warnings), (2, 1));
        assert_eq!(report.messages, ["first", "second", "third"]);
        assert!(!report.is_clean());
    }

    #[test]
    fn information_and_verbose_messages_do_not_count() {
        let log = ValidationLog::default();
        deliver(&log, INFO, &data(c"info".as_ptr()));
        deliver(&log, VERBOSE, &data(c"verbose".as_ptr()));
        assert!(log.report().is_clean());
        assert!(log.report().messages.is_empty());
    }

    #[test]
    fn only_the_first_messages_are_kept_but_every_error_is_counted() {
        let log = ValidationLog::default();
        for _ in 0..MAX_KEPT_MESSAGES + 5 {
            deliver(&log, SEVERITY_ERROR, &data(c"again".as_ptr()));
        }
        let report = log.report();
        assert_eq!(report.messages.len(), MAX_KEPT_MESSAGES);
        assert_eq!(
            usize::try_from(report.errors).ok(),
            Some(MAX_KEPT_MESSAGES + 5)
        );
    }

    #[test]
    fn a_message_that_is_not_utf8_is_kept_with_replacement_characters() {
        let log = ValidationLog::default();
        deliver(&log, SEVERITY_ERROR, &data(c"bad \xff byte".as_ptr()));
        assert!(log.report().messages[0].contains('\u{FFFD}'));
    }

    #[test]
    fn a_null_message_is_recorded_as_such() {
        let log = ValidationLog::default();
        deliver(&log, SEVERITY_WARNING, &data(null()));
        assert_eq!(log.report().messages, ["(no message)"]);
    }

    #[test]
    fn null_pointers_from_the_layer_are_ignored() {
        let log = ValidationLog::default();
        let record = data(c"x".as_ptr());
        // SAFETY: both calls pass a null pointer, which the contract allows.
        let (a, b) = unsafe {
            (
                callback(
                    SEVERITY_ERROR,
                    2,
                    null(),
                    core::ptr::from_ref(&log).cast_mut().cast(),
                ),
                callback(SEVERITY_ERROR, 2, &raw const record, null_mut()),
            )
        };
        assert_eq!((a, b), (0, 0));
        assert!(log.report().is_clean());
    }

    #[test]
    fn a_poisoned_log_still_reports() {
        let log = std::sync::Arc::new(ValidationLog::default());
        let poisoner = std::sync::Arc::clone(&log);
        let _ = std::thread::spawn(move || {
            let _guard = poisoner.0.lock();
            panic!("poison the lock");
        })
        .join();
        log.record(SEVERITY_ERROR, "after");
        assert_eq!(log.report().errors, 1);
    }
}
