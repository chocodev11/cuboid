//! Status line text (spec: app 19.1).

use crate::clock::STATUS_READY_AFTER_S;

/// The text shown when nothing was said for a while.
const READY: &str = "Ready";

/// The text of the status bar's Info cell. A text set at time `t` falls back to "Ready" after
/// [`STATUS_READY_AFTER_S`] seconds.
///
/// spec: app 19.1, ui 4.1
#[derive(Clone, Debug, Default)]
pub struct StatusLine {
    text: String,
    set_at: f64,
}

impl StatusLine {
    /// Shows `text` from time `now` (seconds on the core clock).
    pub fn set(&mut self, text: impl Into<String>, now: f64) {
        self.text = text.into();
        self.set_at = now;
    }

    /// The text to show at time `now`.
    pub fn text(&self, now: f64) -> &str {
        if self.text.is_empty() || now - self.set_at >= f64::from(STATUS_READY_AFTER_S) {
            READY
        } else {
            &self.text
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_message_falls_back_to_ready_after_ten_seconds() {
        let mut status = StatusLine::default();
        assert_eq!(status.text(0.0), "Ready");
        status.set("Saved", 5.0);
        assert_eq!(status.text(5.0), "Saved");
        assert_eq!(status.text(14.9), "Saved");
        assert_eq!(status.text(15.0), "Ready");
    }
}
