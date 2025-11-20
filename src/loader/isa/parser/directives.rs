use std::path::PathBuf;

use crate::soc::isa::ast::{HintBlock, HintComparator, HintDecl, IncludeDecl, IsaItem};
use crate::soc::isa::error::IsaError;
use crate::soc::prog::types::parse_u64_literal;

use super::spans::span_from_tokens;
use super::{
    Parser, TokenKind, parameters::parse_parameter_decl, space::parse_space_directive,
    space_context::parse_space_context_directive,
};

impl<'src> Parser<'src> {
    pub(super) fn parse_directive(&mut self) -> Result<IsaItem, IsaError> {
        self.expect(TokenKind::Colon, "directive introducer ':'")?;
        let name = self.expect_identifier("directive name")?;
        let item = match name.as_str() {
            "fileset" => self.parse_fileset_directive(),
            "param" => self.parse_param_directive(),
            "space" => parse_space_directive(self),
            "hint" => self.parse_hint_directive(),
            "include" => self.parse_include_directive(),
            _ => {
                if self.is_known_space(&name) {
                    self.parse_space_context(&name)
                } else {
                    Err(IsaError::Parser(format!("unsupported directive :{name}")))
                }
            }
        }?;
        self.ensure_directive_boundary(&name)?;
        Ok(item)
    }

    fn parse_fileset_directive(&mut self) -> Result<IsaItem, IsaError> {
        let decl = parse_parameter_decl(self, "fileset parameter name")?;
        Ok(IsaItem::Parameter(decl))
    }

    fn parse_param_directive(&mut self) -> Result<IsaItem, IsaError> {
        let decl = parse_parameter_decl(self, "parameter name")?;
        Ok(IsaItem::Parameter(decl))
    }

    fn parse_include_directive(&mut self) -> Result<IsaItem, IsaError> {
        if !self.allows_include() {
            return Err(IsaError::Parser(
                ":include directive is only allowed inside .coredef files".into(),
            ));
        }
        let path = self.expect(TokenKind::String, "string literal with include path")?;
        Ok(IsaItem::Include(IncludeDecl {
            path: PathBuf::from(path.lexeme),
            optional: false,
        }))
    }

    fn parse_hint_directive(&mut self) -> Result<IsaItem, IsaError> {
        self.expect(TokenKind::LBrace, "'{' to start :hint block")?;
        let mut entries = Vec::new();
        loop {
            if self.check(TokenKind::EOF)? {
                return Err(IsaError::Parser(":hint block missing closing '}'".into()));
            }
            if self.check(TokenKind::RBrace)? {
                self.consume()?;
                break;
            }

            let space_token = self.expect_identifier_token("hint space name")?;
            self.expect(TokenKind::LessThan, "'<' in hint assignment")?;
            self.expect(TokenKind::Dash, "'-' in hint assignment")?;
            let selector = self.expect(TokenKind::BitExpr, "bit expression for hint predicate")?;
            let comparator = self.parse_hint_comparator()?;
            let value_token =
                self.expect(TokenKind::Number, "numeric literal for hint predicate")?;
            let value = parse_u64_literal(&value_token.lexeme).map_err(|err| {
                IsaError::Parser(format!(
                    "invalid numeric literal '{}' for hint predicate: {err}",
                    value_token.lexeme
                ))
            })?;
            let span = span_from_tokens(self.file_path(), &space_token, &value_token);
            entries.push(HintDecl {
                space: space_token.lexeme,
                selector: selector.lexeme,
                comparator,
                value,
                span,
            });

            if self.check(TokenKind::Semicolon)? || self.check(TokenKind::Comma)? {
                self.consume()?;
            }
        }

        if entries.is_empty() {
            return Err(IsaError::Parser(
                ":hint block must declare at least one entry".into(),
            ));
        }

        Ok(IsaItem::Hint(HintBlock { entries }))
    }

    fn parse_hint_comparator(&mut self) -> Result<HintComparator, IsaError> {
        if self.check(TokenKind::Equals)? {
            self.consume()?;
            if self.check(TokenKind::Equals)? {
                self.consume()?;
                return Ok(HintComparator::Equals);
            }
            return Err(IsaError::Parser(
                "hint comparator '=' must be written as '=='".into(),
            ));
        }
        if self.check(TokenKind::Bang)? {
            self.consume()?;
            self.expect(TokenKind::Equals, "'=' after '!' for '!=' comparator")?;
            return Ok(HintComparator::NotEquals);
        }
        Err(IsaError::Parser(
            "hint comparator must be '==' or '!='".into(),
        ))
    }

    fn parse_space_context(&mut self, name: &str) -> Result<IsaItem, IsaError> {
        let kind = self.space_kind(name).ok_or_else(|| {
            IsaError::Parser(format!(
                "space :{name} context referenced before definition"
            ))
        })?;
        parse_space_context_directive(self, name, kind)
    }

    fn ensure_directive_boundary(&mut self, directive: &str) -> Result<(), IsaError> {
        if self.check(TokenKind::EOF)? || self.check(TokenKind::Colon)? {
            return Ok(());
        }

        let mut extras = Vec::new();
        while !self.check(TokenKind::EOF)? {
            if self.check(TokenKind::Colon)? {
                break;
            }
            extras.push(self.consume()?);
        }

        let snippet = extras
            .into_iter()
            .map(|token| token.lexeme)
            .filter(|lex| !lex.is_empty())
            .collect::<Vec<_>>()
            .join(" ");
        let message = if snippet.is_empty() {
            format!("unexpected trailing tokens after :{directive}")
        } else {
            format!("unexpected trailing tokens after :{directive}: {snippet}")
        };
        Err(IsaError::Parser(message))
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use crate::soc::isa::ast::{HintBlock, HintComparator, IsaItem, ParameterDecl, ParameterValue};
    use crate::soc::isa::diagnostic::DiagnosticPhase;
    use crate::soc::isa::error::IsaError;

    use super::super::parse_str;

    fn parse(source: &str) -> crate::soc::isa::ast::IsaSpecification {
        parse_str(PathBuf::from("test.isa"), source).expect("parse")
    }

    fn parse_core(source: &str) -> crate::soc::isa::ast::IsaSpecification {
        parse_str(PathBuf::from("test.coredef"), source).expect("parse")
    }

    fn expect_parser_diag(err: IsaError, needle: &str) {
        match err {
            IsaError::Diagnostics {
                phase: DiagnosticPhase::Parser,
                diagnostics,
            } => {
                assert!(
                    diagnostics.iter().any(|diag| diag.message.contains(needle)),
                    "diagnostics missing needle '{needle}': {diagnostics:?}"
                );
            }
            other => panic!("unexpected error: {other:?}"),
        }
    }

    #[test]
    fn parses_fileset_bitdir_enum() {
        let doc = parse(":fileset BITDIR = LSB0");
        assert_eq!(doc.items.len(), 1, "one parameter expected");
        match &doc.items[0] {
            IsaItem::Parameter(ParameterDecl { name, value }) => {
                assert_eq!(name, "BITDIR");
                assert!(matches!(value, ParameterValue::Word(val) if val == "LSB0"));
            }
            other => panic!("unexpected item: {:?}", other),
        }
    }

    #[test]
    fn parses_fileset_string_literal() {
        let doc = parse(":fileset TAG = \"core\"");
        match &doc.items[0] {
            IsaItem::Parameter(ParameterDecl { name, value }) => {
                assert_eq!(name, "TAG");
                assert!(matches!(value, ParameterValue::Word(val) if val == "core"));
            }
            _ => panic!("expected parameter"),
        }
    }

    #[test]
    fn parses_fileset_number_literal() {
        let doc = parse(":fileset CACHE_SIZE = 0x10");
        match &doc.items[0] {
            IsaItem::Parameter(ParameterDecl { name, value }) => {
                assert_eq!(name, "CACHE_SIZE");
                assert!(matches!(value, ParameterValue::Number(16)));
            }
            _ => panic!("expected parameter"),
        }
    }

    #[test]
    fn parses_param_identifier_literal() {
        let doc = parse(":param ENDIAN = big");
        match &doc.items[0] {
            IsaItem::Parameter(ParameterDecl { name, value }) => {
                assert_eq!(name, "ENDIAN");
                assert!(matches!(value, ParameterValue::Word(val) if val == "big"));
            }
            _ => panic!("expected parameter"),
        }
    }

    #[test]
    fn parses_param_numeric_literal() {
        let doc = parse(":param REGISTER_SIZE = 64");
        match &doc.items[0] {
            IsaItem::Parameter(ParameterDecl { name, value }) => {
                assert_eq!(name, "REGISTER_SIZE");
                assert!(matches!(value, ParameterValue::Number(64)));
            }
            _ => panic!("expected parameter"),
        }
    }

    #[test]
    fn rejects_unknown_directive() {
        let err = parse_str(PathBuf::from("test.isa"), ":unknown foo").unwrap_err();
        expect_parser_diag(err, "unsupported directive");
    }

    #[test]
    fn parses_basic_space_context() {
        let doc = parse(
            ":space reg addr=32 word=64 type=register\n:reg GPR size=64 subfields={\n    VALUE @(0..63)\n}",
        );
        assert_eq!(doc.items.len(), 2, "space + member item expected");
        assert!(matches!(doc.items[0], IsaItem::Space(_)));
        match &doc.items[1] {
            IsaItem::SpaceMember(member) => {
                assert_eq!(member.space, "reg");
            }
            other => panic!("unexpected item: {other:?}"),
        }
    }

    #[test]
    fn errors_on_trailing_tokens_after_directive() {
        let err = parse_str(PathBuf::from("test.isa"), ":param ENDIAN=big extra").unwrap_err();
        expect_parser_diag(err, "unexpected trailing tokens");
    }

    #[test]
    fn parser_reports_multiple_errors() {
        let source = ":param FOO=1 extra\n:param BAR=2 extra";
        let err = parse_str(PathBuf::from("test.isa"), source).unwrap_err();
        match err {
            IsaError::Diagnostics {
                phase: DiagnosticPhase::Parser,
                diagnostics,
            } => {
                assert!(
                    diagnostics.len() >= 2,
                    "expected multiple diagnostics: {diagnostics:?}"
                );
                let trailing = diagnostics
                    .iter()
                    .filter(|diag| diag.message.contains("unexpected trailing tokens"))
                    .count();
                assert!(
                    trailing >= 2,
                    "expected two trailing token diagnostics: {diagnostics:?}"
                );
            }
            other => panic!("unexpected error: {other:?}"),
        }
    }

    #[test]
    fn parses_hint_block_single_entry() {
        let doc = parse(":hint { code <- @(0..3)==0x1 }");
        assert_eq!(doc.items.len(), 1);
        match &doc.items[0] {
            IsaItem::Hint(HintBlock { entries }) => {
                assert_eq!(entries.len(), 1);
                let hint = &entries[0];
                assert_eq!(hint.space, "code");
                assert_eq!(hint.selector, "@(0..3)");
                assert_eq!(hint.value, 1);
                assert!(matches!(hint.comparator, HintComparator::Equals));
            }
            other => panic!("unexpected item: {other:?}"),
        }
    }

    #[test]
    fn parses_hint_block_multiple_entries() {
        let doc = parse(":hint { a <- @(0..3)==0, b <- @(0..3)!=0 }");
        match &doc.items[0] {
            IsaItem::Hint(HintBlock { entries }) => {
                assert_eq!(entries.len(), 2);
                assert!(matches!(entries[1].comparator, HintComparator::NotEquals));
            }
            other => panic!("unexpected item: {other:?}"),
        }
    }

    #[test]
    fn parses_include_in_coredef() {
        let doc = parse_core(":include \"base.isa\"");
        assert_eq!(doc.items.len(), 1);
        match &doc.items[0] {
            IsaItem::Include(include) => {
                assert_eq!(include.path, PathBuf::from("base.isa"));
                assert!(!include.optional);
            }
            other => panic!("unexpected item: {other:?}"),
        }
    }

    #[test]
    fn rejects_include_outside_coredef() {
        let err = parse_str(PathBuf::from("test.isa"), ":include \"base.isa\"")
            .unwrap_err();
        expect_parser_diag(err, "only allowed inside .coredef");
    }
}
