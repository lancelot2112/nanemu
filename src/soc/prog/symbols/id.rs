//! Identifier helpers used by the symbol subsystem.

use std::fmt;
use std::num::{NonZeroU32, NonZeroU64};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct SymbolId(NonZeroU64);

impl SymbolId {
    pub fn new(value: NonZeroU64) -> Self {
        Self(value)
    }

    pub fn from_u64(value: u64) -> Option<Self> {
        NonZeroU64::new(value).map(Self)
    }

    pub fn get(self) -> NonZeroU64 {
        self.0
    }
}

impl fmt::Display for SymbolId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "#{}", self.0)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct SymbolHandle(u32);

impl SymbolHandle {
    pub fn from_index(index: usize) -> Self {
        assert!(
            index < (u32::MAX as usize),
            "SymbolHandle index exceeded u32::MAX range"
        );
        Self(index as u32)
    }

    pub fn index(self) -> usize {
        self.0 as usize
    }

    pub fn raw(self) -> u32 {
        self.0
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct LabelId(NonZeroU32);

impl LabelId {
    pub(crate) fn from_index(index: usize) -> Self {
        let raw = NonZeroU32::new((index as u32) + 1).expect("label index overflow");
        Self(raw)
    }

    pub fn index(self) -> usize {
        (self.0.get() - 1) as usize
    }
}

#[cfg(test)]
mod tests {
    //! Ensures identifiers enforce their invariants and formatting helpers stay predictable.
    use super::*;

    #[test]
    fn symbol_id_rejects_zero_values() {
        assert!(
            SymbolId::from_u64(0).is_none(),
            "Zero must remain a reserved sentinel"
        );
    }

    #[test]
    fn symbol_handle_round_trips_indices() {
        let handle = SymbolHandle::from_index(5);
        assert_eq!(
            handle.index(),
            5,
            "Dense handle indices are required for cache-friendly lookups"
        );
    }

    #[test]
    fn label_id_encodes_dense_space() {
        let label = LabelId::from_index(3);
        assert_eq!(
            label.index(),
            3,
            "LabelId should map indices without gaps to preserve interning determinism"
        );
    }

    #[test]
    fn symbol_id_displays_with_hash_prefix() {
        let nz = NonZeroU64::new(7).expect("non-zero");
        let sid = SymbolId::new(nz);
        assert_eq!(
            sid.to_string(),
            "#7",
            "Display impl should expose the raw numeric identifier"
        );
    }
}
