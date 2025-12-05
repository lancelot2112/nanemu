//! Shared utilities for decoding symbol-backed type records into high-level values.

use crate::soc::bus::BusCursor;
use crate::soc::bus::ext::{BitsCursorExt, FloatCursorExt, SignedCursorExt, StringCursorExt};
use crate::soc::prog::symbols::walker::SymbolWalkEntry;
use crate::soc::prog::types::arena::TypeArena;
use crate::soc::prog::types::bitfield::BitFieldSpec;
use crate::soc::prog::types::pointer::PointerType;
use crate::soc::prog::types::record::TypeRecord;
use crate::soc::prog::types::scalar::{EnumType, FixedScalar, ScalarEncoding, ScalarType};

use super::value::{SymbolAccessError, SymbolValue};

pub struct ReadContext<'ctx, 'arena> {
    pub data: &'ctx mut BusCursor,
    pub arena: &'arena TypeArena,
    pub entry: Option<&'ctx SymbolWalkEntry>,
    pub field_address: usize,
    pub symbol_base: usize,
    pub size_hint: Option<usize>,
}

impl<'ctx, 'arena> ReadContext<'ctx, 'arena> {
    pub fn new(
        data: &'ctx mut BusCursor,
        arena: &'arena TypeArena,
        entry: Option<&'ctx SymbolWalkEntry>,
        field_address: usize,
        symbol_base: usize,
        size_hint: Option<usize>,
    ) -> Self {
        Self {
            data,
            arena,
            entry,
            field_address,
            symbol_base,
            size_hint,
        }
    }
}

pub trait SymbolReadable {
    fn read_symbol_value<'ctx>(
        &self,
        ctx: &mut ReadContext<'ctx, '_>,
    ) -> Result<Option<SymbolValue<'ctx>>, SymbolAccessError>;
}

impl SymbolReadable for ScalarType {
    fn read_symbol_value<'ctx>(
        &self,
        ctx: &mut ReadContext<'ctx, '_>,
    ) -> Result<Option<SymbolValue<'ctx>>, SymbolAccessError> {
        ctx.data.goto(ctx.field_address)?;
        let value = match self.encoding {
            ScalarEncoding::Unsigned => {
                if self.bit_size > 64 {
                    return Ok(None);
                }
                let value = self.read_unsigned(ctx.data)? as u64;
                Some(SymbolValue::Unsigned(value))
            }
            ScalarEncoding::Signed => {
                if self.bit_size > 64 {
                    return Ok(None);
                }
                let value = self.read_signed(ctx.data)? as i64;
                Some(SymbolValue::Signed(value))
            }
            ScalarEncoding::Floating => match self.byte_size {
                4 => {
                    let value = ctx.data.read_f32()?;
                    Some(SymbolValue::Float(value as f64))
                }
                8 => {
                    let value = ctx.data.read_f64()?;
                    Some(SymbolValue::Float(value))
                }
                _ => None,
            },
            ScalarEncoding::Utf8String => {
                if self.byte_size == 0 {
                    return Ok(Some(SymbolValue::Utf8(String::new())));
                }
                let value = ctx.data.read_utf8(self.byte_size)?;
                Some(SymbolValue::Utf8(value))
            }
        };
        Ok(value)
    }
}

impl SymbolReadable for EnumType {
    fn read_symbol_value<'ctx>(
        &self,
        ctx: &mut ReadContext<'ctx, '_>,
    ) -> Result<Option<SymbolValue<'ctx>>, SymbolAccessError> {
        ctx.data.goto(ctx.field_address)?;
        if self.base.bit_size > 64 {
            return Ok(None);
        }
        let value = self.base.read_signed(ctx.data)? as i64;
        let label = self
            .label_for(value)
            .map(|id| ctx.arena.resolve_string(id).to_string());
        Ok(Some(SymbolValue::Enum { label, value }))
    }
}

impl SymbolReadable for FixedScalar {
    fn read_symbol_value<'ctx>(
        &self,
        ctx: &mut ReadContext<'ctx, '_>,
    ) -> Result<Option<SymbolValue<'ctx>>, SymbolAccessError> {
        ctx.data.goto(ctx.field_address)?;
        if self.base.bit_size == 0 {
            return Ok(Some(SymbolValue::Float(self.apply(0))));
        }
        if self.base.bit_size > 64 {
            return Ok(None);
        }
        let raw = self.base.read_signed(ctx.data)? as i64;
        Ok(Some(SymbolValue::Float(self.apply(raw))))
    }
}

impl SymbolReadable for PointerType {
    fn read_symbol_value<'ctx>(
        &self,
        ctx: &mut ReadContext<'ctx, '_>,
    ) -> Result<Option<SymbolValue<'ctx>>, SymbolAccessError> {
        ctx.data.goto(ctx.field_address)?;
        let width = self.byte_size.max(ctx.size_hint.unwrap_or(self.byte_size)) as usize;
        if width > 8 {
            return Ok(None);
        }
        let value = if width == 0 {
            0
        } else {
            ctx.data.read_bits(0, width * 8)? as u64
        };
        Ok(Some(SymbolValue::Unsigned(value)))
    }
}

impl SymbolReadable for BitFieldSpec {
    fn read_symbol_value<'ctx>(
        &self,
        ctx: &mut ReadContext<'ctx, '_>,
    ) -> Result<Option<SymbolValue<'ctx>>, SymbolAccessError> {
        let width = self.total_width();
        if width == 0 {
            return Ok(Some(SymbolValue::Unsigned(0)));
        }
        if width > 64 {
            return Err(SymbolAccessError::UnsupportedTraversal {
                label: "bitfield wider than 64 bits".into(),
            });
        }
        ctx.data.goto(ctx.field_address)?;
        
        let (raw_value, actual_width) = self.read_bits(container_bits);
        debug_assert_eq!(self.total_width(), actual_width);
        let value = if self.is_signed() {
            let shift = 64 - actual_width;
            let signed = ((raw_value << shift) as i64) >> shift;
            SymbolValue::Signed(signed)
        } else {
            SymbolValue::Unsigned(raw_value)
        };
        Ok(Some(value))
    }
}

pub fn read_type_record<'ctx>(
    record: &TypeRecord,
    ctx: &mut ReadContext<'ctx, '_>,
) -> Result<Option<SymbolValue<'ctx>>, SymbolAccessError> {
    match record {
        TypeRecord::Scalar(scalar) => scalar.read_symbol_value(ctx),
        TypeRecord::Enum(enum_type) => enum_type.read_symbol_value(ctx),
        TypeRecord::Fixed(fixed) => fixed.read_symbol_value(ctx),
        TypeRecord::Pointer(pointer) => pointer.read_symbol_value(ctx),
        TypeRecord::BitField(bitfield) => bitfield.read_symbol_value(ctx),
        _ => Ok(None),
    }
}
