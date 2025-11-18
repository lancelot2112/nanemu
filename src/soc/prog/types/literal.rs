//! Numeric literal parser shared between ISA parser, symbol tooling, and other components.

use std::fmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Literal {
    value: u64,
    kind: LiteralKind,
    bit_width: Option<u16>,
}

impl Literal {
    pub fn value(&self) -> u64 {
        self.value
    }

    pub fn kind(&self) -> LiteralKind {
        self.kind
    }

    /// Returns the declared bit width for binary literals (based on the digit count).
    pub fn bit_width(&self) -> Option<u16> {
        self.bit_width
    }

    pub fn parse(input: &str) -> Result<Self, LiteralError> {
        let trimmed = input.trim();
        if trimmed.is_empty() {
            return Err(LiteralError::Empty);
        }
        if let Some(rest) = trimmed.strip_prefix("0b") {
            return Self::parse_binary(rest);
        }
        if let Some(rest) = trimmed.strip_prefix("0o") {
            return Self::parse_radix(rest, 8, LiteralKind::Octal);
        }
        if let Some(rest) = trimmed.strip_prefix("0x") {
            return Self::parse_radix(rest, 16, LiteralKind::Hex);
        }
        Self::parse_radix(trimmed, 10, LiteralKind::Decimal)
    }

    fn parse_binary(src: &str) -> Result<Self, LiteralError> {
        let digits = src.replace('_', "");
        if digits.is_empty() {
            return Err(LiteralError::InvalidFormat("0b".into()));
        }
        if digits.len() > 64 {
            return Err(LiteralError::TooWide {
                bits: digits.len() as u16,
            });
        }
        let value = u64::from_str_radix(&digits, 2)
            .map_err(|_| LiteralError::InvalidFormat(format!("0b{}", src)))?;
        Ok(Literal {
            value,
            kind: LiteralKind::Binary,
            bit_width: Some(digits.len() as u16),
        })
    }

    fn parse_radix(src: &str, radix: u32, kind: LiteralKind) -> Result<Self, LiteralError> {
        let digits = src.replace('_', "");
        if digits.is_empty() {
            return Err(LiteralError::InvalidFormat(src.into()));
        }
        let value = u64::from_str_radix(&digits, radix)
            .map_err(|_| LiteralError::InvalidFormat(src.into()))?;
        Ok(Literal {
            value,
            kind,
            bit_width: None,
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LiteralKind {
    Decimal,
    Hex,
    Octal,
    Binary,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LiteralError {
    Empty,
    InvalidFormat(String),
    TooWide { bits: u16 },
}

impl fmt::Display for LiteralError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            LiteralError::Empty => write!(f, "literal is empty"),
            LiteralError::InvalidFormat(token) => write!(f, "invalid literal format: {token}"),
            LiteralError::TooWide { bits } => write!(f, "binary literal width {bits} exceeds 64 bits"),
        }
    }
}

impl std::error::Error for LiteralError {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_decimal_literal() {
        let literal = Literal::parse("42").expect("literal parse");
        assert_eq!(literal.value(), 42);
        assert_eq!(literal.kind(), LiteralKind::Decimal);
        assert_eq!(literal.bit_width(), None);
    }

    #[test]
    fn parses_hex_literal() {
        let literal = Literal::parse("0xFF").expect("literal parse");
        assert_eq!(literal.value(), 255);
        assert_eq!(literal.kind(), LiteralKind::Hex);
    }

    #[test]
    fn parses_binary_literal_and_width() {
        let literal = Literal::parse("0b1010").expect("literal parse");
        assert_eq!(literal.value(), 0b1010);
        assert_eq!(literal.bit_width(), Some(4));
        assert_eq!(literal.kind(), LiteralKind::Binary);
    }

    #[test]
    fn rejects_wide_binary() {
        let wide = "0b".to_string() + &"1".repeat(65);
        assert!(Literal::parse(&wide).is_err(), "wide binary should fail");
    }
}
