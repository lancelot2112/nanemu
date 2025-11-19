# Parser Architecture

The parser is structured as a set of focused modules that cooperate through the `Parser` type defined in `document.rs`.

- `document.rs`
  - Owns the `Parser` struct, cursor helpers (`peek`, `consume`, `expect`, etc.), and `parse_document` loop that walks the token stream.
  - Exposes the ergonomic `parse_str` helper the loader uses when it only needs a one shot parse.
  - Collects structured diagnostics (code, message, source span) and resynchronizes at the next directive after each error so later directives keep parsing.
- `directives.rs`
  - Dispatches to directive-specific modules via extension `impl`s on `Parser`.
  - Guarantees that each directive consumes everything up to the next `:`; leftover tokens are collected and surfaced as parser errors so individual handlers stay simple.
  - Hosts tests for the shared directive infrastructure plus lightweight cases for directives implemented inline (e.g., `:param`).
- `space.rs`
  - Contains the full `:space` parser, including attribute validation, numeric literal decoding, and space tag registration.
  - Includes its own tests that exercise success/error cases so regressions stay localized.
- `space_context.rs`
  - Handles `:<space_tag>` contexts for non-logic spaces, parsing register forms, attribute lists, and nested `subfields={}` blocks.
  - Enforces redirect-only fields (no `offset`/`size`/`reset` when `redirect=` is present) so register definitions stay within the spec without waiting for a later validation pass.
  - Reuses the shared `Parser` helpers plus numeric literal routines to keep directive-specific logic focused on structure rather than token management.
- `parameters.rs`
  - Shared routines for decoding directive payloads into `ParameterDecl` values.
  - Leans on helper methods from `Parser` for token management and the shared literal utilities in `soc::prog::types::literal` when numbers are encountered.

`mod.rs` remains intentionally small: it wires the modules together, re-exports the public API, and exposes the shared lexer token types to the submodules. Adding a new directive normally involves editing only `directives.rs` (for the syntax) and, if necessary, extending `parameters.rs` or the shared literal helpers for reusable parsing logic.
