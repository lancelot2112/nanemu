#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Endianness {
    Little,
    Big,
}

impl Endianness {
    pub const fn native() -> Self {
        if cfg!(target_endian = "little") {
            Endianness::Little
        } else {
            Endianness::Big
        }
    }

    pub(crate) fn read_u16(self, bytes: [u8; 2]) -> u16 {
        match self {
            Endianness::Little => u16::from_le_bytes(bytes),
            Endianness::Big => u16::from_be_bytes(bytes),
        }
    }

    pub(crate) fn read_u32(self, bytes: [u8; 4]) -> u32 {
        match self {
            Endianness::Little => u32::from_le_bytes(bytes),
            Endianness::Big => u32::from_be_bytes(bytes),
        }
    }

    pub(crate) fn read_u64(self, bytes: [u8; 8]) -> u64 {
        match self {
            Endianness::Little => u64::from_le_bytes(bytes),
            Endianness::Big => u64::from_be_bytes(bytes),
        }
    }

    pub(crate) fn write_u16(self, value: u16) -> [u8; 2] {
        match self {
            Endianness::Little => value.to_le_bytes(),
            Endianness::Big => value.to_be_bytes(),
        }
    }

    pub(crate) fn write_u32(self, value: u32) -> [u8; 4] {
        match self {
            Endianness::Little => value.to_le_bytes(),
            Endianness::Big => value.to_be_bytes(),
        }
    }

    pub(crate) fn write_u64(self, value: u64) -> [u8; 8] {
        match self {
            Endianness::Little => value.to_le_bytes(),
            Endianness::Big => value.to_be_bytes(),
        }
    }

    pub(crate) fn read_bytes(self, bytes: &[u8]) -> Vec<u8> {
        match self {
            Endianness::Little => bytes.iter().rev().cloned().collect(),
            Endianness::Big => bytes.to_vec(),
        }
    }

    pub(crate) fn write_bytes(self, bytes: &[u8]) -> Vec<u8> {
        match self {
            Endianness::Little => bytes.iter().rev().cloned().collect(),
            Endianness::Big => bytes.to_vec(),
        }
    }
}
