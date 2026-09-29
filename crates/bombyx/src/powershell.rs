//! Text bombyx hands to Windows PowerShell on a Windows guest.
//!
//! Every value and script bombyx sends a Windows guest crosses
//! several shells on the way: the VM host's `sh`, vagrant's own
//! wrapping, and PowerShell's parser. Base64 holds no character any
//! of them reads, so this module encodes what crosses.

/// The standard base64 alphabet, RFC 4648 section 4.
const ALPHABET: &[u8; 64] =
    b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

/// `bytes` in standard base64, padded with `=`, on one line.
///
/// Written out rather than taken from a crate: it is a dozen lines,
/// and a dependency would cost a cooldown and a licence review for
/// them. PowerShell's `[Convert]::FromBase64String` reads exactly
/// this form, and so does Ruby's `unpack("m0")`.
pub(crate) fn base64(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let b = [
            chunk[0],
            chunk.get(1).copied().unwrap_or(0),
            chunk.get(2).copied().unwrap_or(0),
        ];
        let n = u32::from(b[0]) << 16 | u32::from(b[1]) << 8 | u32::from(b[2]);
        // Each index is six bits, so it is always inside ALPHABET.
        let sextet =
            |shift: u32| char::from(ALPHABET[(n >> shift & 63) as usize]);
        out.push(sextet(18));
        out.push(sextet(12));
        out.push(if chunk.len() > 1 { sextet(6) } else { '=' });
        out.push(if chunk.len() > 2 { sextet(0) } else { '=' });
    }
    out
}

/// PowerShell text that runs `script` in the session reading it:
/// `script`'s UTF-8 bytes in base64, decoded and passed to
/// `Invoke-Expression`. The text holds no `'`, and it grows by a
/// third where `-EncodedCommand`'s UTF-16 would double it.
pub(crate) fn run_encoded(script: &str) -> String {
    format!(
        "iex ([Text.Encoding]::UTF8.GetString(\
         [Convert]::FromBase64String(\"{}\")))",
        base64(script.as_bytes())
    )
}

/// `script` without its blank lines and whole-line `#` comments, so
/// the text a guest's command line carries stays short. It keeps a
/// `#` inside a line, and it does not know about here-strings, so
/// `script` must hold none.
pub(crate) fn code_lines(script: &str) -> String {
    script
        .lines()
        .filter(|line| {
            let line = line.trim();
            !line.is_empty() && !line.starts_with('#')
        })
        .fold(String::new(), |mut out, line| {
            out.push_str(line);
            out.push('\n');
            out
        })
}

/// `value` as a PowerShell single-quoted string, which expands
/// nothing; a `'` inside it is doubled, as PowerShell reads one.
pub(crate) fn quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "''"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_encoded_script_runs_through_invoke_expression() {
        // `ab'c` is YWInYw== in base64: the quote travels inside it.
        assert_eq!(
            run_encoded("ab'c"),
            "iex ([Text.Encoding]::UTF8.GetString(\
             [Convert]::FromBase64String(\"YWInYw==\")))"
        );
    }

    #[test]
    fn code_lines_drop_comments_and_blank_lines_only() {
        let script = "# header\n\n$a = 1  # kept\n    # indented\n\
                      Write-Host '#not a comment'\n\t\n";
        assert_eq!(
            code_lines(script),
            "$a = 1  # kept\nWrite-Host '#not a comment'\n"
        );
    }

    #[test]
    fn a_quoted_value_doubles_its_single_quotes() {
        assert_eq!(quote("agent"), "'agent'");
        assert_eq!(quote("it's"), "'it''s'");
        assert_eq!(quote(""), "''");
    }

    #[test]
    fn base64_matches_the_rfc_4648_test_vectors() {
        // RFC 4648 section 10, which covers every padding case.
        for (plain, encoded) in [
            ("", ""),
            ("f", "Zg=="),
            ("fo", "Zm8="),
            ("foo", "Zm9v"),
            ("foob", "Zm9vYg=="),
            ("fooba", "Zm9vYmE="),
            ("foobar", "Zm9vYmFy"),
        ] {
            assert_eq!(base64(plain.as_bytes()), encoded, "{plain:?}");
        }
    }

    #[test]
    fn base64_uses_the_two_symbols_of_the_standard_alphabet() {
        // 0xfb 0xff encodes to `+/8=`: the last two alphabet
        // entries, which a URL-safe variant would spell `-_`.
        assert_eq!(base64(&[0xfb, 0xff]), "+/8=");
    }

    #[test]
    fn base64_holds_no_character_powershell_reads_in_double_quotes() {
        let all: Vec<u8> = (0..=255).collect();
        let out = base64(&all);
        assert!(
            out.chars()
                .all(|c| c.is_ascii_alphanumeric()
                    || matches!(c, '+' | '/' | '=')),
            "{out}"
        );
    }
}
