//! Decimal-string → signed minor-unit (2 dp) parsing.

/// Parse a decimal money string into SIGNED minor units (hundredths).
/// Handles surrounding whitespace, thousands separators (','), a leading
/// '+'/'-', and accounting parentheses (e.g. "(50.00)" => -5000).
/// Rejects empty strings, non-numeric input, and more than 2 decimal places.
pub fn parse_decimal_to_minor(raw: &str) -> Result<i64, String> {
    let s = raw.trim();
    if s.is_empty() {
        return Err("empty amount".into());
    }
    let (neg_paren, s) = if s.starts_with('(') && s.ends_with(')') {
        (true, &s[1..s.len() - 1])
    } else {
        (false, s)
    };
    let s = s.trim().replace(',', "");
    let (sign, digits) = match s.strip_prefix('-') {
        Some(rest) => (-1i64, rest),
        None => (1i64, s.strip_prefix('+').unwrap_or(&s)),
    };
    if neg_paren && sign == -1 {
        return Err(format!("ambiguous sign (both '-' and parentheses): {raw}"));
    }
    if digits.is_empty() {
        return Err(format!("not a number: {raw}"));
    }
    let (int_part, frac_part) = match digits.split_once('.') {
        Some((i, f)) => (i, f),
        None => (digits, ""),
    };
    if frac_part.len() > 2 {
        return Err(format!("more than 2 decimal places: {raw}"));
    }
    let int_part = if int_part.is_empty() { "0" } else { int_part };
    if !int_part.bytes().all(|b| b.is_ascii_digit())
        || !frac_part.bytes().all(|b| b.is_ascii_digit())
    {
        return Err(format!("not a number: {raw}"));
    }
    let whole: i64 = int_part.parse().map_err(|_| format!("not a number: {raw}"))?;
    let frac: i64 = format!("{frac_part:0<2}").parse().unwrap_or(0);
    let magnitude = whole
        .checked_mul(100)
        .and_then(|v| v.checked_add(frac))
        .ok_or_else(|| format!("amount out of range: {raw}"))?;
    let signed = magnitude * sign * if neg_paren { -1 } else { 1 };
    Ok(signed)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_plain() {
        assert_eq!(parse_decimal_to_minor("123.45").unwrap(), 12345);
    }
    #[test]
    fn parses_integer() {
        assert_eq!(parse_decimal_to_minor("100").unwrap(), 10000);
    }
    #[test]
    fn parses_one_decimal() {
        assert_eq!(parse_decimal_to_minor("12.5").unwrap(), 1250);
    }
    #[test]
    fn parses_thousands_separators() {
        assert_eq!(parse_decimal_to_minor("1,234.56").unwrap(), 123456);
    }
    #[test]
    fn parses_parens_as_negative() {
        assert_eq!(parse_decimal_to_minor("(50.00)").unwrap(), -5000);
    }
    #[test]
    fn parses_leading_minus() {
        assert_eq!(parse_decimal_to_minor("-7.00").unwrap(), -700);
    }
    #[test]
    fn rejects_empty() {
        assert!(parse_decimal_to_minor("   ").is_err());
    }
    #[test]
    fn rejects_three_decimals() {
        assert!(parse_decimal_to_minor("1.234").is_err());
    }
    #[test]
    fn rejects_garbage() {
        assert!(parse_decimal_to_minor("abc").is_err());
    }
    #[test]
    fn rejects_overflow() {
        assert!(parse_decimal_to_minor("99999999999999999").is_err());
    }
    #[test]
    fn rejects_double_negative() {
        assert!(parse_decimal_to_minor("(-50.00)").is_err());
    }
}
