use std::fmt;

/// A line-oriented text writer whose lines end in LF or NL, chosen per line.
///
/// A literal `\n` of the original is an LF. Its line writer ends lines with the platform
/// newline, CRLF on the Windows build; `cubik29` writes CRLF for it on every platform, so the
/// NL here is always `\r\n` and the files do not depend on the host. OBJ and MTL files mix both.
/// The text is UTF-8 without a byte-order mark.
///
/// `write!` appends formatted text; it cannot fail.
///
/// spec: interop 3.3
#[derive(Clone, Debug, Default)]
pub struct Lines {
    out: String,
}

impl Lines {
    /// An empty text.
    pub fn new() -> Self {
        Self::default()
    }

    /// Appends text without ending the line.
    pub fn push_str(&mut self, text: &str) {
        self.out.push_str(text);
    }

    /// Ends the line with LF.
    pub fn lf(&mut self) {
        self.out.push('\n');
    }

    /// Ends the line with NL, which is CRLF.
    pub fn nl(&mut self) {
        self.out.push_str("\r\n");
    }

    /// Appends `text` and ends the line with LF.
    pub fn lf_line(&mut self, text: &str) {
        self.push_str(text);
        self.lf();
    }

    /// Appends `text` and ends the line with NL.
    pub fn nl_line(&mut self, text: &str) {
        self.push_str(text);
        self.nl();
    }

    /// The text so far.
    pub fn as_str(&self) -> &str {
        &self.out
    }

    /// Ends the text and returns it.
    pub fn into_string(self) -> String {
        self.out
    }
}

impl fmt::Write for Lines {
    fn write_str(&mut self, text: &str) -> fmt::Result {
        self.out.push_str(text);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::fmt::Write;

    use super::*;

    #[test]
    fn lines_end_in_lf_or_in_crlf_as_asked() {
        let mut text = Lines::new();
        text.nl_line("# Export of Cubik Model: Box");
        text.lf_line("v 0 0 0");
        write!(text, "f {} {} {}", 1, 2, 3).unwrap();
        text.nl();
        text.push_str("last");
        text.lf();
        assert_eq!(
            text.as_str(),
            "# Export of Cubik Model: Box\r\nv 0 0 0\nf 1 2 3\r\nlast\n"
        );
    }

    #[test]
    fn the_text_has_no_byte_order_mark_and_keeps_non_ascii() {
        let mut text = Lines::new();
        text.lf_line("é 😀");
        assert_eq!(text.into_string().as_bytes(), "é 😀\n".as_bytes());
    }
}
