//! Standalone entry point for the ISA language server.
//!
//! Launch with `cargo run --features language-server --bin isa_language_server` or point your
//! editor's LSP client to the compiled binary.

#[cfg(not(feature = "language-server"))]
pub fn main() {
    eprintln!(
        "The 'isa_language_server' binary requires the 'language-server' feature. \
Enable it with `cargo run --features language-server --bin isa_language_server`."
    );
    std::process::exit(1);
}

#[cfg(feature = "language-server")]
#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    use nanemu::loader::isa::extension::run_stdio_language_server;

    run_stdio_language_server().await?;
    Ok(())
}
