//! The portable ECMA-262 subset, translated once for Rust regex consumers.
//! Disabling Unicode globally would break UTF-8 complements and lose ECMA's
//! whitespace set, so only word boundaries use Rust's ASCII mode.

const SPACE: &str =
    r"\t-\r \x{00A0}\x{1680}\x{2000}-\x{200A}\x{2028}\x{2029}\x{202F}\x{205F}\x{3000}\x{FEFF}";

pub(crate) fn rust(pattern: &str) -> Result<String, String> {
    let mut chars = pattern.chars().peekable();
    let mut out = String::new();
    let mut class = false;
    while let Some(ch) = chars.next() {
        match ch {
            '\\' => {
                let escaped = chars.next().ok_or("trailing pattern escape")?;
                match escaped {
                    'd' => out.push_str("[0-9]"),
                    'D' => out.push_str("[^0-9]"),
                    'w' => out.push_str("[A-Za-z0-9_]"),
                    'W' => out.push_str("[^A-Za-z0-9_]"),
                    's' | 'S' => {
                        out.push('[');
                        if escaped == 'S' {
                            out.push('^');
                        }
                        out.push_str(SPACE);
                        out.push(']');
                    }
                    'b' if class => out.push_str(r"\x08"),
                    'b' | 'B' if !class => {
                        out.push_str("(?-u:\\");
                        out.push(escaped);
                        out.push(')');
                    }
                    'u' | 'x' => {
                        let count = if escaped == 'u' { 4 } else { 2 };
                        let mut hex = String::new();
                        for _ in 0..count {
                            let digit = chars
                                .next()
                                .filter(char::is_ascii_hexdigit)
                                .ok_or("only fixed-width ECMA hexadecimal escapes are supported")?;
                            hex.push(digit);
                        }
                        let value = u32::from_str_radix(&hex, 16).expect("hex digits checked");
                        if (0xd800..=0xdfff).contains(&value) {
                            return Err(
                                "UTF-16 surrogate escapes cannot be represented by Rust regex"
                                    .into(),
                            );
                        }
                        out.push_str(&format!("\\x{{{hex}}}"));
                    }
                    'c' => {
                        let letter = chars
                            .next()
                            .filter(char::is_ascii_alphabetic)
                            .ok_or("a control escape requires an ASCII letter")?;
                        out.push_str(&format!(
                            "\\x{{{:02X}}}",
                            u32::from(letter.to_ascii_uppercase()) % 32
                        ));
                    }
                    '0' if chars.peek().is_none_or(|c| !c.is_ascii_digit()) => {
                        out.push_str(r"\x00")
                    }
                    'n' | 'r' | 't' | 'f' | 'v' => {
                        out.push('\\');
                        out.push(escaped);
                    }
                    // Identity escapes vary with ECMA flags; refuse extensions
                    // such as Rust's \A and \p rather than changing their meaning.
                    c if c.is_ascii_alphanumeric() => {
                        return Err(format!("unsupported ECMA escape \\{c}"));
                    }
                    '<' | '>' => {
                        return Err(format!("unsupported ECMA identity escape \\{escaped}"));
                    }
                    c => {
                        out.push('\\');
                        out.push(c);
                    }
                }
            }
            '[' if class => out.push_str(r"\["),
            '[' => {
                // ECMA closes an empty class immediately; Rust treats that
                // closing bracket as a literal, changing the following tokens.
                if chars.peek() == Some(&']') {
                    chars.next();
                    out.push_str(r"[^\x{0}-\x{10FFFF}]");
                    continue;
                }
                let mut lookahead = chars.clone();
                if lookahead.next() == Some('^') && lookahead.next() == Some(']') {
                    chars.next();
                    chars.next();
                    out.push_str(r"[\x{0}-\x{10FFFF}]");
                    continue;
                }
                class = true;
                out.push(ch);
            }
            ']' if class => {
                class = false;
                out.push(ch);
            }
            '.' if !class => out.push_str(r"[^\n\r\x{2028}\x{2029}]"),
            '(' if !class && chars.peek() == Some(&'?') => {
                chars.next();
                if chars.next() != Some(':') {
                    return Err("only ordinary and noncapturing ECMA groups are supported (no flags, look-around or named groups)".into());
                }
                out.push_str("(?:");
            }
            // ECMA classes do not have Rust's intersection/difference operators.
            '&' | '~' if class => {
                out.push('\\');
                out.push(ch);
            }
            '-' if class && chars.peek() == Some(&'-') => {
                return Err(
                    "ambiguous doubled hyphens in a character class are not supported".into(),
                );
            }
            ']' if !class => out.push_str(r"\]"),
            c => out.push(c),
        }
    }
    if class {
        return Err("unterminated character class".into());
    }
    Ok(out)
}
