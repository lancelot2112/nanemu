//! Formatting helpers for debugging and telemetry logs.

use std::fmt;

use super::id::SymbolHandle;
use super::table::SymbolTable;

pub struct SymbolFormatter<'a> {
    table: &'a SymbolTable,
    handle: SymbolHandle,
}

impl<'a> SymbolFormatter<'a> {
    pub fn new(table: &'a SymbolTable, handle: SymbolHandle) -> Self {
        Self { table, handle }
    }
}

impl<'a> fmt::Display for SymbolFormatter<'a> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let record = self.table.get(self.handle);
        write!(f, "{}", self.table.resolve_label(record.label))?;
        write!(f, " | {:?}", record.kind)?;
        if let Some(addr) = record.runtime_addr {
            write!(f, " @0x{addr:08X}")?;
        }
        if let Some(size) = record.size {
            write!(f, " [{} bytes]", size)?;
        }
        Ok(())
    }
}

pub fn describe_symbol<'a>(table: &'a SymbolTable, handle: SymbolHandle) -> SymbolFormatter<'a> {
    SymbolFormatter::new(table, handle)
}

#[cfg(test)]
mod tests {
    //! Ensures formatting surfaces the critical metadata consumers expect.
    use std::sync::Arc;

    use crate::soc::prog::symbols::table::SymbolTable;
    use crate::soc::prog::types::TypeArena;

    use super::*;

    #[test]
    fn formatter_includes_label_and_addresses() {
        let types = Arc::new(TypeArena::new());
        let mut table = SymbolTable::new(types);
        let handle = table
            .builder()
            .label("ANGLE")
            .runtime_addr(0x1234)
            .size(8)
            .finish();
        let rendered = describe_symbol(&table, handle).to_string();
        assert!(
            rendered.contains("ANGLE"),
            "Formatted string should mention the label for quick identification"
        );
        assert!(
            rendered.contains("0x00001234"),
            "Runtime address should be rendered in padded hex form"
        );
        assert!(
            rendered.contains("8 bytes"),
            "Size metadata must be visible to callers"
        );
    }
}
