//! Turning driver values into display text.

use crate::model::Cell;

/// Binary data beyond this is summarised instead of hex-dumped.
const MAX_BLOB_PREVIEW: usize = 2048;

pub fn hex_blob(bytes: &[u8]) -> String {
    let shown = &bytes[..bytes.len().min(MAX_BLOB_PREVIEW)];
    let hex: String = shown.iter().map(|b| format!("{b:02X}")).collect();
    if shown.len() < bytes.len() {
        format!("0x{hex}… ({} bytes)", bytes.len())
    } else {
        format!("0x{hex}")
    }
}

pub fn text_or_hex(bytes: Option<Vec<u8>>) -> Cell {
    bytes.map(|b| match String::from_utf8(b) {
        Ok(s) => s,
        Err(e) => hex_blob(e.as_bytes()),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hex_dumps_small_blobs() {
        assert_eq!(hex_blob(&[0x00, 0xAB, 0x10]), "0x00AB10");
    }

    #[test]
    fn summarises_large_blobs() {
        let s = hex_blob(&vec![1u8; MAX_BLOB_PREVIEW + 5]);
        assert!(s.ends_with(&format!("… ({} bytes)", MAX_BLOB_PREVIEW + 5)));
    }

    #[test]
    fn text_or_hex_prefers_utf8() {
        assert_eq!(text_or_hex(Some(b"hi".to_vec())).as_deref(), Some("hi"));
        assert_eq!(
            text_or_hex(Some(vec![0xFF, 0xFE])).as_deref(),
            Some("0xFFFE")
        );
        assert_eq!(text_or_hex(None), None);
    }
}
