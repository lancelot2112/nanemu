# Loader ISA Diagnostics

This note explains the diagnostic pipeline that runs while loading an ISA file. It captures how the loader converts raw text into actionable `IsaDiagnostic` records and where state lives so language-servers and CLI tools can show precise spans.

## Pipeline overview

1. **Lexer phase** (`lexer.rs`). Characters are tokenized and any trivia-level failures (bad characters, malformed numeric prefixes, unterminated strings, etc.) call `emit_lexer_diagnostic`. Each diagnostic carries:
   - `phase = DiagnosticPhase::Lexer`
   - A short code such as `lexer.string.unterminated`
   - A point span derived from the lexer's tracked `(line, column)` and file `PathBuf`

2. **Parser phase** (`parser/*.rs`). Once tokens exist, structural mistakes (unknown directives, missing identifiers, bad attribute lists, etc.) are reported through helper methods on `Parser`. These diagnostics use `DiagnosticPhase::Parser` and leverage `span_from_token` so the error underlines the exact token that triggered it.

3. **Validation phase** (`soc/isa/validator.rs`). After a document parses successfully, semantic rules run (duplicate names, redirect misuse, cross-space lookups). These diagnostics are emitted with `DiagnosticPhase::Validation` and usually span the offending AST node.

Each stage returns an `IsaError::Diagnostics` variant that bundles the phase and a vector of `IsaDiagnostic`s, allowing callers to aggregate or short-circuit as needed.

## Adding a new diagnostic

1. **Pick a code** that scopes naturally (e.g., `lexer.number.missing-digits`).
2. **Capture an accurate span**. The lexer uses `SourceSpan::point` while the parser/validator typically compute multi-token spans.
3. **Bubble it up via `IsaError::Diagnostics`** so higher layers do not need to parse strings.
4. **Test it**. Lexer unit tests assert against `Err(IsaError::Diagnostics { .. })` and parser/validator tests inspect the collected `IsaDiagnostic`s.

Keeping codes, spans, and phases consistent is what lets editor integrations show rich squiggles without reverse engineering string messages.

## Why `src/soc/isa/diagnostic.rs`?

Even though the loader emits the majority of diagnostics, the data model is shared by:

- `loader/isa/lexer.rs`
- `loader/isa/parser/*`
- `soc/isa/validator.rs`
- Any future tooling that wants to surface ISA errors (e.g., IDE plugins or machine builders)

Because these consumers live outside the loader module, the diagnostic types sit at the `soc::isa` layer alongside `IsaError`. That keeps the definitions reusable without circular dependencies and matches how other shared ISA structures (AST, validator helpers, etc.) are organized.
