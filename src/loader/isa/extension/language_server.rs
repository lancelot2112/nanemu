use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use tokio::sync::RwLock;
use tower_lsp::jsonrpc::Result as LspResult;
use tower_lsp::lsp_types::{
    Diagnostic, DiagnosticRelatedInformation, DiagnosticSeverity, InitializeParams,
    InitializeResult, InitializedParams, MessageType, NumberOrString, Position, Range,
    ServerCapabilities, ServerInfo, TextDocumentSyncCapability, TextDocumentSyncKind,
    TextDocumentSyncOptions, Url,
};
use tower_lsp::{Client, LanguageServer, LspService, Server, async_trait};

use crate::loader::isa::parse_str;
use crate::soc::isa::diagnostic::{DiagnosticLevel, IsaDiagnostic, SourceSpan};
use crate::soc::isa::error::IsaError;

const SUPPORTED_EXTENSIONS: &[&str] = &["isa", "isaext", "coredef", "sysdef"];

#[derive(Clone)]
struct DocumentEntry {
    text: String,
    version: i32,
}

/// Minimal LSP backend that surfaces parser diagnostics for ISA family documents.
pub struct IsaLanguageServer {
    client: Client,
    documents: Arc<RwLock<HashMap<Url, DocumentEntry>>>,
}

impl IsaLanguageServer {
    pub fn new(client: Client) -> Self {
        Self {
            client,
            documents: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    async fn refresh_document(&self, uri: &Url) {
        if !Self::is_supported_uri(uri) {
            return;
        }
        let snapshot = {
            let docs = self.documents.read().await;
            docs.get(uri).cloned()
        };
        if let Some(entry) = snapshot {
            let diagnostics = self.parse_document(uri, &entry.text);
            self.client
                .publish_diagnostics(uri.clone(), diagnostics, Some(entry.version))
                .await;
        }
    }

    async fn remove_document(&self, uri: &Url) {
        self.documents.write().await.remove(uri);
        self.client
            .publish_diagnostics(uri.clone(), Vec::new(), None)
            .await;
    }

    fn parse_document(&self, uri: &Url, text: &str) -> Vec<Diagnostic> {
        if !Self::is_supported_uri(uri) {
            return Vec::new();
        }
        match parse_str(Self::path_from_uri(uri), text) {
            Ok(_) => Vec::new(),
            Err(err) => Self::diagnostics_from_error(uri, err),
        }
    }

    fn diagnostics_from_error(uri: &Url, err: IsaError) -> Vec<Diagnostic> {
        match err {
            IsaError::Diagnostics { diagnostics, .. } => diagnostics
                .into_iter()
                .map(|diag| Self::isa_diag_to_lsp(uri, diag))
                .collect(),
            other => vec![Self::message_only_diagnostic(&other.to_string())],
        }
    }

    fn isa_diag_to_lsp(uri: &Url, diag: IsaDiagnostic) -> Diagnostic {
        let IsaDiagnostic {
            phase,
            level,
            code,
            message,
            span,
        } = diag;
        let range = span.as_ref().map(Self::range_from_span).unwrap_or_default();
        let severity = Some(match level {
            DiagnosticLevel::Error => DiagnosticSeverity::ERROR,
            DiagnosticLevel::Warning => DiagnosticSeverity::WARNING,
        });
        Diagnostic {
            range,
            severity,
            code: Some(NumberOrString::String(code.into())),
            source: Some(format!("{:?}", phase)),
            message,
            related_information: span.as_ref().and_then(|span| {
                if Self::matches_uri(uri, span) {
                    None
                } else {
                    Some(vec![DiagnosticRelatedInformation {
                        location: tower_lsp::lsp_types::Location::new(
                            Url::from_file_path(&span.path).unwrap_or_else(|_| uri.clone()),
                            Self::range_from_span(span),
                        ),
                        message: format!("referenced in {}", span.path.display()),
                    }])
                }
            }),
            ..Diagnostic::default()
        }
    }

    fn message_only_diagnostic(message: &str) -> Diagnostic {
        Diagnostic {
            range: Range::default(),
            severity: Some(DiagnosticSeverity::ERROR),
            message: message.to_string(),
            source: Some("nanemu-isa".into()),
            ..Diagnostic::default()
        }
    }

    fn range_from_span(span: &SourceSpan) -> Range {
        Range {
            start: Position {
                line: span.start.line.saturating_sub(1) as u32,
                character: span.start.column.saturating_sub(1) as u32,
            },
            end: Position {
                line: span.end.line.saturating_sub(1) as u32,
                character: span.end.column.saturating_sub(1) as u32,
            },
        }
    }

    fn path_from_uri(uri: &Url) -> PathBuf {
        uri.to_file_path()
            .unwrap_or_else(|_| PathBuf::from(uri.path()))
    }

    fn is_supported_uri(uri: &Url) -> bool {
        Self::path_from_uri(uri)
            .extension()
            .and_then(|ext| ext.to_str())
            .map(|ext| {
                let ext = ext.to_ascii_lowercase();
                SUPPORTED_EXTENSIONS.contains(&ext.as_str())
            })
            .unwrap_or(false)
    }

    fn matches_uri(uri: &Url, span: &SourceSpan) -> bool {
        if let Ok(span_uri) = Url::from_file_path(&span.path) {
            uri == &span_uri
        } else {
            false
        }
    }
}

#[async_trait]
impl LanguageServer for IsaLanguageServer {
    async fn initialize(&self, _: InitializeParams) -> LspResult<InitializeResult> {
        let capabilities = ServerCapabilities {
            text_document_sync: Some(TextDocumentSyncCapability::Options(
                TextDocumentSyncOptions {
                    open_close: Some(true),
                    change: Some(TextDocumentSyncKind::FULL),
                    ..Default::default()
                },
            )),
            ..ServerCapabilities::default()
        };
        Ok(InitializeResult {
            capabilities,
            server_info: Some(ServerInfo {
                name: "nanemu-isa".into(),
                version: Some(env!("CARGO_PKG_VERSION").into()),
            }),
        })
    }

    async fn initialized(&self, _: InitializedParams) {
        self.client
            .log_message(MessageType::INFO, "ISA language server initialized")
            .await;
    }

    async fn shutdown(&self) -> LspResult<()> {
        Ok(())
    }

    async fn did_open(&self, params: tower_lsp::lsp_types::DidOpenTextDocumentParams) {
        let doc = params.text_document;
        if !Self::is_supported_uri(&doc.uri) {
            return;
        }
        self.documents.write().await.insert(
            doc.uri.clone(),
            DocumentEntry {
                text: doc.text.clone(),
                version: doc.version,
            },
        );
        self.refresh_document(&doc.uri).await;
    }

    async fn did_change(&self, params: tower_lsp::lsp_types::DidChangeTextDocumentParams) {
        if params.content_changes.is_empty() {
            return;
        }
        let uri = params.text_document.uri;
        if !Self::is_supported_uri(&uri) {
            return;
        }
        let new_text = params
            .content_changes
            .last()
            .map(|change| change.text.clone())
            .unwrap_or_default();
        self.documents.write().await.insert(
            uri.clone(),
            DocumentEntry {
                text: new_text,
                version: params.text_document.version,
            },
        );
        self.refresh_document(&uri).await;
    }

    async fn did_close(&self, params: tower_lsp::lsp_types::DidCloseTextDocumentParams) {
        self.remove_document(&params.text_document.uri).await;
    }
}

async fn run_stdio_language_server_impl<F>(factory: F) -> LspResult<()>
where
    F: Fn(Client) -> IsaLanguageServer,
{
    let stdin = tokio::io::stdin();
    let stdout = tokio::io::stdout();
    let (service, socket) = LspService::build(|client| factory(client)).finish();
    Server::new(stdin, stdout, socket).serve(service).await;
    Ok(())
}

pub async fn run_stdio_language_server() -> LspResult<()> {
    run_stdio_language_server_impl(IsaLanguageServer::new).await
}
