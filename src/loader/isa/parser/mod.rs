//! Recursive descent parser that turns lexer tokens into [`IsaDocument`](crate::soc::isa::ast::IsaDocument).

mod directives;
mod document;
mod literals;
mod parameters;
mod space;
mod space_context;
mod spans;

pub use document::{Parser, parse_str};

pub(super) use super::lexer::{Lexer, Token, TokenKind};
