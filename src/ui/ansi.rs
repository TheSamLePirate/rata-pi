//! Tiny, dependency-free ANSI-escape stripper.
//!
//! Pi's tool output (especially bash) can contain CSI / SGR / OSC sequences. Until
//! we bring in a proper terminal-colour-preserving renderer (M5), we just strip so
//! the text renders cleanly without noise bleeding into Ratatui styling.

pub fn strip(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let bytes = s.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        let b = bytes[i];
        if b == 0x1b && i + 1 < bytes.len() {
            let next = bytes[i + 1];
            match next {
                b'[' => {
                    // CSI: ESC [ … @~
                    i += 2;
                    while i < bytes.len() && !(0x40..=0x7e).contains(&bytes[i]) {
                        i += 1;
                    }
                    if i < bytes.len() {
                        i += 1;
                    }
                }
                b']' => {
                    // OSC: ESC ] … (ST = ESC \ or BEL)
                    i += 2;
                    while i < bytes.len() {
                        if bytes[i] == 0x07 {
                            i += 1;
                            break;
                        }
                        if bytes[i] == 0x1b && i + 1 < bytes.len() && bytes[i + 1] == b'\\' {
                            i += 2;
                            break;
                        }
                        i += 1;
                    }
                }
                b'P' | b'X' | b'^' | b'_' => {
                    // DCS/SOS/PM/APC: ESC [PX^_] … ST
                    i += 2;
                    while i < bytes.len() {
                        if bytes[i] == 0x1b && i + 1 < bytes.len() && bytes[i + 1] == b'\\' {
                            i += 2;
                            break;
                        }
                        i += 1;
                    }
                }
                _ => {
                    // two-char escape
                    i += 2;
                }
            }
            continue;
        }
        // UTF-8 safe append
        let ch_len = utf8_len(b);
        if i + ch_len <= bytes.len() {
            out.push_str(std::str::from_utf8(&bytes[i..i + ch_len]).unwrap_or(""));
            i += ch_len;
        } else {
            i += 1;
        }
    }
    out
}

fn utf8_len(first: u8) -> usize {
    // ASCII and continuation bytes both advance by 1; multi-byte leads advance
    // by the sequence length.
    if first < 0xC0 {
        1
    } else if first < 0xE0 {
        2
    } else if first < 0xF0 {
        3
    } else {
        4
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strips_basic_sgr() {
        assert_eq!(strip("\x1b[31mred\x1b[0m"), "red");
    }

    #[test]
    fn strips_complex_csi() {
        assert_eq!(strip("hello \x1b[1;32mworld\x1b[0m!"), "hello world!");
    }

    #[test]
    fn strips_osc_title() {
        assert_eq!(strip("\x1b]0;title\x07body"), "body");
        assert_eq!(strip("\x1b]8;;https://x\x1b\\link\x1b]8;;\x1b\\"), "link");
    }

    #[test]
    fn keeps_plain_text_and_unicode() {
        assert_eq!(strip("café 🎉"), "café 🎉");
    }

    #[test]
    fn strips_clear_screen() {
        assert_eq!(strip("\x1b[2J\x1b[Hhello"), "hello");
    }
}
