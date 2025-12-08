//! Language-server integration for ISA-related definition files.
//!
//! The code in this module is gated behind the `language-server` crate feature because it pulls in
//! async dependencies (`tokio`, `tower-lsp`) that are unnecessary for the default build.

#[cfg(feature = "language-server")]
mod language_server;

#[cfg(feature = "language-server")]
pub use language_server::{IsaLanguageServer, run_stdio_language_server};
