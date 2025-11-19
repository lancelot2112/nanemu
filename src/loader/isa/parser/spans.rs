use std::path::Path;

use crate::soc::isa::diagnostic::{SourcePosition, SourceSpan};

use super::Token;

pub fn span_from_tokens(path: &Path, start: &Token, end: &Token) -> SourceSpan {
    SourceSpan::new(
        path.to_path_buf(),
        SourcePosition::new(start.line, start.column),
        token_end_position(end),
    )
}

pub fn span_from_token(path: &Path, token: &Token) -> SourceSpan {
    span_from_tokens(path, token, token)
}

fn token_end_position(token: &Token) -> SourcePosition {
    let mut line = token.line;
    let mut column = token.column;
    if token.lexeme.is_empty() {
        return SourcePosition::new(line, column);
    }
    for ch in token.lexeme.chars() {
        if ch == '\n' {
            line += 1;
            column = 1;
        } else {
            column += 1;
        }
    }
    SourcePosition::new(line, column)
}
