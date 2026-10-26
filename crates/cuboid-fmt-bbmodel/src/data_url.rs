//! `data:` URLs with base64 payloads, the form in which a project embeds its textures.

pub use cuboid_io::text::base64::{decode, encode};

/// The base64 payload of an image `data:` URL (`data:image/png;base64,...`), or `None` when the
/// text is not one.
pub fn payload(url: &str) -> Option<&str> {
    let rest = url.trim_start().strip_prefix("data:")?;
    let (head, body) = rest.split_once(',')?;
    let mut parts = head.split(';');
    let mime = parts.next()?.trim();
    (mime.starts_with("image/") && parts.any(|p| p.trim().eq_ignore_ascii_case("base64")))
        .then_some(body)
}

/// A PNG as the `data:` URL a project stores.
pub fn png_url(png: &[u8]) -> String {
    format!("data:image/png;base64,{}", encode(png))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn urls_are_split_into_the_payload() {
        assert_eq!(payload("data:image/png;base64,AAAA"), Some("AAAA"));
        assert_eq!(
            payload("data:image/jpeg;charset=x;base64,AAAA"),
            Some("AAAA")
        );
        assert_eq!(payload("data:text/plain;base64,AAAA"), None);
        assert_eq!(payload("data:image/png,AAAA"), None);
        assert_eq!(payload("file:///a.png"), None);
        assert_eq!(png_url(b"foo"), "data:image/png;base64,Zm9v");
    }
}
