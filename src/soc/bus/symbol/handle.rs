//! Symbol-aware handle that links the program symbol table with live device bus reads.

use std::sync::Arc;

use crate::soc::bus::{BusCursor, DeviceBus};
use crate::soc::device::AccessContext;
use crate::soc::prog::symbols::walker::SymbolWalker;
use crate::soc::prog::symbols::{
    SymbolHandle as TableSymbolHandle, SymbolId, SymbolRecord, SymbolTable,
};
use crate::soc::prog::types::arena::{TypeArena, TypeId};

use super::cursor::SymbolValueCursor;
use super::read::{ReadContext, read_type_record};
use super::size::type_size;
use super::value::{SymbolAccessError, SymbolValue};

/// Computes typed values for symbols by combining the symbol table with a live bus view.
pub struct SymbolHandle<'a> {
    pub(super) table: &'a SymbolTable,
    pub(super) cursor: BusCursor,
}

impl<'a> SymbolHandle<'a> {
    pub fn new(table: &'a SymbolTable, bus: Arc<DeviceBus>) -> Self {
        Self {
            table,
            cursor: BusCursor::attach_to_bus(bus, 0, AccessContext::DEBUG),
        }
    }

    pub fn resolve_label(&self, label: &str) -> Option<TableSymbolHandle> {
        self.table
            .lookup_label(label)
            .and_then(|id| self.table.handles_by_label(id))
            .and_then(|handles| handles.first().copied())
    }

    pub fn resolve_symbol_id(&self, id: SymbolId) -> Option<TableSymbolHandle> {
        self.table.handle_by_symbol_id(id)
    }

    /// Creates a cursor that walks all primitive values reachable from the symbol's type tree.
    pub fn value_cursor<'handle>(
        &'handle mut self,
        symbol: TableSymbolHandle,
    ) -> Result<SymbolValueCursor<'handle, 'a>, SymbolAccessError> {
        let snapshot = self.prepare(symbol)?;
        let Some(type_id) = snapshot.record.type_id else {
            let label = self.table.resolve_label(snapshot.record.label).to_string();
            return Err(SymbolAccessError::UnsupportedTraversal { label });
        };
        let arena = self.table.type_arena();
        let walker = SymbolWalker::new(arena.as_ref(), type_id);
        Ok(SymbolValueCursor {
            handle: self,
            snapshot,
            walker,
            arena: arena.as_ref(),
        })
    }

    pub fn read_value(
        &mut self,
        symbol: TableSymbolHandle,
    ) -> Result<SymbolValue, SymbolAccessError> {
        let snapshot = self.prepare(symbol)?;
        let arena = self.table.type_arena();
        if let Some(value) = self.interpret_value(arena.as_ref(), &snapshot)? {
            return Ok(value);
        }
        let bytes = self.read_bytes(&snapshot)?;
        Ok(SymbolValue::Bytes(bytes))
    }

    pub fn read_raw_bytes(
        &mut self,
        symbol: TableSymbolHandle,
    ) -> Result<&[u8], SymbolAccessError> {
        let snapshot = self.prepare(symbol)?;
        self.read_bytes(&snapshot)
    }

    fn prepare(&self, symbol: TableSymbolHandle) -> Result<Snapshot, SymbolAccessError> {
        let record = self.table.get(symbol).clone();
        let label = self.table.resolve_label(record.label).to_string();
        let address =
            record
                .runtime_addr
                .or(record.file_addr)
                .ok_or(SymbolAccessError::MissingAddress {
                    label: label.clone(),
                })?;
        let size = record
            .size
            .or_else(|| {
                record
                    .type_id
                    .and_then(|ty| type_size(self.table.type_arena().as_ref(), ty))
            })
            .ok_or(SymbolAccessError::MissingSize { label })?;
        Ok(Snapshot {
            record,
            address,
            size,
        })
    }

    fn read_bytes(&mut self, snapshot: &Snapshot) -> Result<&[u8], SymbolAccessError> {
        self.cursor.goto(snapshot.address)?;
        let buf = self.cursor.read_ram(snapshot.size as usize)?;
        Ok(buf)
    }

    fn interpret_value(
        &mut self,
        arena: &TypeArena,
        snapshot: &Snapshot,
    ) -> Result<Option<SymbolValue>, SymbolAccessError> {
        let Some(type_id) = snapshot.record.type_id else {
            return Ok(None);
        };
        self.interpret_type_at(arena, type_id, snapshot.address, Some(snapshot.size))
    }

    pub(super) fn interpret_type_at(
        &mut self,
        arena: &TypeArena,
        type_id: TypeId,
        address: usize,
        size_hint: Option<usize>,
    ) -> Result<Option<SymbolValue>, SymbolAccessError> {
        let record = arena.get(type_id);
        let mut ctx = ReadContext::new(&mut self.cursor, arena, None, address, address, size_hint);
        read_type_record(record, &mut ctx)
    }
}

pub(super) struct Snapshot {
    pub record: SymbolRecord,
    pub address: usize,
    pub size: usize,
}
