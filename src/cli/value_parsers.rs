//! Pure CLI value coercion shared by startup and runtime services.

/// Coerce the case-insensitive boolean spellings accepted by Claude Code.
pub fn parse_prompt_suggestions(raw: &str) -> Result<bool, String> {
    match raw.to_ascii_lowercase().as_str() {
        "true" | "1" | "yes" | "on" => Ok(true),
        "false" | "0" | "no" | "off" => Ok(false),
        _ => Err(format!(
            "error: option '--prompt-suggestions [value]' argument '{raw}' is invalid. Allowed choices are true, false, 1, 0, yes, no, on, off."
        )),
    }
}

/// The window may exceed 1M in 2.1.332. Small bare numbers are k shorthand;
/// suffixed values can be fractional. Stay within JavaScript's safe integers.
pub fn parse_autocompact_value(raw: &str) -> Result<Option<i64>, String> {
    let invalid = || {
        format!(
            "error: option '--autocompact <auto|tokens>' argument '{raw}' is invalid. It must be 'auto', or at least 100k (e.g. 500k, 200000, or 200 as shorthand)"
        )
    };
    let value = raw.trim();
    if value.eq_ignore_ascii_case("auto") {
        return Ok(None);
    }
    let lower = value.to_ascii_lowercase();
    let (number, multiplier) = if let Some(number) = lower.strip_suffix('k') {
        (number.trim(), 1_000.0)
    } else if let Some(number) = lower.strip_suffix('m') {
        (number.trim(), 1_000_000.0)
    } else {
        (lower.as_str(), 1.0)
    };
    // The upstream coercer accepts a numeric prefix (JavaScript parseFloat),
    // including an exponent, rather than requiring the whole value to parse.
    // Only a three-digit grouping may be removed. Removing arbitrary or
    // repeated separators would wrongly accept 1,00, 10_00, and 1,,00.
    let mut number = number.to_string();
    if let Some(index) = number.find([',', '_', ' ']) {
        let bytes = number.as_bytes();
        if bytes
            .get(index + 1..index + 4)
            .is_some_and(|group| group.iter().all(u8::is_ascii_digit))
            && !bytes.get(index + 4).is_some_and(u8::is_ascii_digit)
        {
            number.remove(index);
        }
    }
    let prefix = parse_decimal_prefix(&number).ok_or_else(invalid)?;
    // Truncate large literal token counts before f64 conversion; otherwise
    // 9007199254740991.9 rounds out of JS's safe-integer range before flooring.
    if multiplier == 1.0
        && !prefix.contains(['e', 'E'])
        && let Some((integer, _)) = prefix.split_once('.')
        && let Ok(tokens) = integer.parse::<u64>()
        && tokens >= 100_000
    {
        return if tokens <= 9_007_199_254_740_991 {
            Ok(Some(tokens as i64))
        } else {
            Err(invalid())
        };
    }
    let number = prefix.parse::<f64>().map_err(|_| invalid())?;
    let number = if multiplier == 1.0 && (100.0..1001.0).contains(&number) {
        number * 1_000.0
    } else {
        number * multiplier
    };
    if !number.is_finite() || !(100_000.0..=9_007_199_254_740_991.0).contains(&number) {
        return Err(invalid());
    }
    Ok(Some(number.floor() as i64))
}

/// Decimal-prefix conversion without accepting Rust-only infinity spellings or
/// hexadecimal numbers. Incomplete exponents do not belong to the prefix.
fn parse_decimal_prefix(raw: &str) -> Option<&str> {
    let bytes = raw.as_bytes();
    let mut end = usize::from(matches!(bytes.first(), Some(b'+' | b'-')));
    let start = end;
    while bytes.get(end).is_some_and(u8::is_ascii_digit) {
        end += 1;
    }
    let mut digits = end - start;
    if bytes.get(end) == Some(&b'.') {
        end += 1;
        let fraction = end;
        while bytes.get(end).is_some_and(u8::is_ascii_digit) {
            end += 1;
        }
        digits += end - fraction;
    }
    if digits == 0 {
        return None;
    }
    if matches!(bytes.get(end), Some(b'e' | b'E')) {
        let exponent = end;
        end += 1;
        if matches!(bytes.get(end), Some(b'+' | b'-')) {
            end += 1;
        }
        let exponent_digits = end;
        while bytes.get(end).is_some_and(u8::is_ascii_digit) {
            end += 1;
        }
        if end == exponent_digits {
            end = exponent;
        }
    }
    Some(&raw[..end])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn suggestions_accept_boolean_aliases_case_insensitively() {
        for value in ["true", "TRUE", "1", "yes", "on"] {
            assert_eq!(parse_prompt_suggestions(value), Ok(true), "{value}");
        }
        for value in ["false", "False", "0", "no", "off"] {
            assert_eq!(parse_prompt_suggestions(value), Ok(false), "{value}");
        }
        for value in ["", "bogus", " true ", "2"] {
            assert!(parse_prompt_suggestions(value).is_err(), "{value}");
        }
    }

    #[test]
    fn compact_window_accepts_latest_documented_values() {
        for value in ["auto", "AUTO", " auto "] {
            assert_eq!(parse_autocompact_value(value), Ok(None), "{value}");
        }
        for (value, tokens) in [
            ("100", 100_000),
            ("1000", 1_000_000),
            ("500k", 500_000),
            ("100.5k", 100_500),
            ("0.1M", 100_000),
            ("1.5m", 1_500_000),
            ("2M", 2_000_000),
            ("2000000", 2_000_000),
            ("1000001", 1_000_001),
            ("1e5", 100_000),
            ("1,000", 1_000_000),
            ("100_000", 100_000),
        ] {
            assert_eq!(parse_autocompact_value(value), Ok(Some(tokens)), "{value}");
        }
        for value in [
            "",
            "bogus",
            "99k",
            "99",
            "1001",
            "99999.9",
            "NaN",
            "Infinity",
            "9007199254740992",
        ] {
            assert!(parse_autocompact_value(value).is_err(), "{value}");
        }
    }
}
