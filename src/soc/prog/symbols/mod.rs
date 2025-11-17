//! Entry point for the `soc::prog::symbols` subsystem.

pub mod builder;
pub mod fmt;
pub mod id;
pub mod query;
pub mod source;
pub mod symbol;
pub mod table;
pub mod walker;

pub use builder::SymbolBuilder;
pub use fmt::{describe_symbol, SymbolFormatter};
pub use id::{LabelId, SymbolHandle, SymbolId};
pub use query::{SymbolQuery, SymbolQueryIter};
pub use source::{SourceTrust, SymbolProvenance, SymbolSource};
pub use symbol::{
    StorageClass, SymbolBinding, SymbolInfo, SymbolKind, SymbolRecord, SymbolState, SymbolVisibility,
    ToolFlags,
};
pub use table::SymbolTable;
pub use walker::{SymbolPath, SymbolWalkEntry, SymbolWalker, ValueKind};

#[cfg(test)]
mod tests {
    //! Surface-level smoke test proving the module wiring works end-to-end.
    use std::sync::Arc;

    use crate::soc::prog::types::TypeArena;

    use super::*;

    #[test]
    fn module_reexports_allow_builder_usage() {
        let types = Arc::new(TypeArena::new());
        let mut table = SymbolTable::new(types);
        let handle = table.builder().label("TEMP").finish();
        assert_eq!(table.len(), 1, "Module should expose a working builder via re-export");
        let rendered = describe_symbol(&table, handle).to_string();
        assert!(rendered.contains("TEMP"), "Formatter re-export should be usable from the root module");
    }
}
