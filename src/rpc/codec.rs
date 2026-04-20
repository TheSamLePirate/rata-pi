//! Strict JSONL framing for the pi RPC protocol.
//!
//! From `pi docs/rpc.md`:
//! > RPC mode uses strict JSONL semantics with LF (`\n`) as the only record delimiter.
//! > Split records on `\n` only. Accept optional `\r\n` input by stripping a trailing `\r`.
//! > Do not use generic line readers that treat Unicode separators as newlines.
//!
//! Node's `readline` is explicitly called out as non-compliant because it also splits on
//! U+2028 / U+2029. This codec scans for `\n` bytes only — the Unicode separators pass
//! through untouched inside JSON strings.
//!
//! Oversized-line handling: rather than buffering unbounded amounts, we cap at
//! `max_line_bytes` (default 16 MiB — generous for large tool outputs) and return a typed
//! error instead of panicking. The caller can choose to log and resync.

// Wired up in M1.
#![allow(dead_code)]

use std::io;

use bytes::{Buf, BytesMut};
use tokio_util::codec::{Decoder, Encoder};

#[derive(Debug, thiserror::Error)]
pub enum JsonlError {
    #[error("io: {0}")]
    Io(#[from] io::Error),
    #[error("line exceeded max size ({limit} bytes)")]
    Oversize { limit: usize },
    #[error("invalid utf-8 in rpc line")]
    Utf8(#[from] std::string::FromUtf8Error),
}

pub struct JsonlCodec {
    max_line_bytes: usize,
    next_index: usize,
}

impl JsonlCodec {
    pub fn new(max_line_bytes: usize) -> Self {
        Self {
            max_line_bytes,
            next_index: 0,
        }
    }
}

impl Default for JsonlCodec {
    fn default() -> Self {
        Self::new(16 * 1024 * 1024)
    }
}

impl Decoder for JsonlCodec {
    type Item = String;
    type Error = JsonlError;

    fn decode(&mut self, src: &mut BytesMut) -> Result<Option<Self::Item>, Self::Error> {
        if src.len() > self.max_line_bytes && !src[..self.max_line_bytes].contains(&b'\n') {
            // Recover by dropping the oversized prefix; surface a typed error.
            let drop = src.len();
            src.advance(drop);
            self.next_index = 0;
            return Err(JsonlError::Oversize {
                limit: self.max_line_bytes,
            });
        }

        if let Some(rel) = src[self.next_index..].iter().position(|&b| b == b'\n') {
            let newline_at = self.next_index + rel;
            let mut line = src.split_to(newline_at + 1);
            line.truncate(line.len() - 1); // drop '\n'
            if line.last() == Some(&b'\r') {
                line.truncate(line.len() - 1); // tolerate trailing '\r'
            }
            self.next_index = 0;
            Ok(Some(String::from_utf8(line.to_vec())?))
        } else {
            self.next_index = src.len();
            Ok(None)
        }
    }

    fn decode_eof(&mut self, src: &mut BytesMut) -> Result<Option<Self::Item>, Self::Error> {
        match self.decode(src)? {
            Some(line) => Ok(Some(line)),
            None if src.is_empty() => Ok(None),
            None => {
                let mut line = src.split_to(src.len());
                if line.last() == Some(&b'\r') {
                    line.truncate(line.len() - 1);
                }
                self.next_index = 0;
                Ok(Some(String::from_utf8(line.to_vec())?))
            }
        }
    }
}

impl Encoder<&str> for JsonlCodec {
    type Error = JsonlError;
    fn encode(&mut self, item: &str, dst: &mut BytesMut) -> Result<(), Self::Error> {
        dst.reserve(item.len() + 1);
        dst.extend_from_slice(item.as_bytes());
        dst.extend_from_slice(b"\n");
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn drain(codec: &mut JsonlCodec, bytes: &[u8]) -> Vec<String> {
        let mut buf = BytesMut::from(bytes);
        let mut out = Vec::new();
        while let Ok(Some(line)) = codec.decode(&mut buf) {
            out.push(line);
        }
        out
    }

    #[test]
    fn splits_on_lf() {
        let mut c = JsonlCodec::default();
        assert_eq!(drain(&mut c, b"a\nb\nc\n"), vec!["a", "b", "c"]);
    }

    #[test]
    fn strips_trailing_cr() {
        let mut c = JsonlCodec::default();
        assert_eq!(drain(&mut c, b"hello\r\nworld\r\n"), vec!["hello", "world"]);
    }

    #[test]
    fn does_not_split_on_unicode_separators() {
        // U+2028 = E2 80 A8, U+2029 = E2 80 A9 — legal inside JSON strings, MUST NOT split.
        let mut c = JsonlCodec::default();
        let input = b"\"a\xe2\x80\xa8b\xe2\x80\xa9c\"\n";
        let out = drain(&mut c, input);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].as_bytes(), b"\"a\xe2\x80\xa8b\xe2\x80\xa9c\"");
    }

    #[test]
    fn chunked_input_resumes_scan_from_next_index() {
        let mut c = JsonlCodec::default();
        let mut buf = BytesMut::new();
        buf.extend_from_slice(b"par");
        assert!(matches!(c.decode(&mut buf), Ok(None)));
        buf.extend_from_slice(b"tial\n");
        assert_eq!(c.decode(&mut buf).unwrap(), Some("partial".to_string()));
    }

    #[test]
    fn decode_eof_returns_trailing_fragment_without_newline() {
        let mut c = JsonlCodec::default();
        let mut buf = BytesMut::from(&b"last"[..]);
        assert_eq!(c.decode_eof(&mut buf).unwrap(), Some("last".to_string()));
        assert_eq!(c.decode_eof(&mut buf).unwrap(), None);
    }

    #[test]
    fn oversize_line_returns_error_and_recovers() {
        let mut c = JsonlCodec::new(8);
        let mut buf = BytesMut::from(&b"way-too-long-without-newline-yet"[..]);
        assert!(matches!(
            c.decode(&mut buf),
            Err(JsonlError::Oversize { limit: 8 })
        ));
        // After recovery, fresh input decodes normally.
        buf.extend_from_slice(b"ok\n");
        assert_eq!(c.decode(&mut buf).unwrap(), Some("ok".to_string()));
    }

    #[test]
    fn encode_appends_newline() {
        let mut c = JsonlCodec::default();
        let mut buf = BytesMut::new();
        c.encode("{\"x\":1}", &mut buf).unwrap();
        assert_eq!(buf.as_ref(), b"{\"x\":1}\n");
    }
}
