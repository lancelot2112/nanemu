#[cfg_attr(not(test), allow(dead_code))]
pub const MAX_ENDIAN_BYTES: usize = 16;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Endianness {
    Little,
    Big,
}

impl Endianness {
    #[inline(always)]
    pub const fn native() -> Self {
        if cfg!(target_endian = "little") {
            Endianness::Little
        } else {
            Endianness::Big
        }
    }

    #[inline(always)]
    pub fn decode_bytes(self, bytes: &[u8]) -> u128 {
        assert!(bytes.len() <= MAX_ENDIAN_BYTES, "value exceeds 128 bits");
        if bytes.is_empty() {
            return 0;
        }
        let mut buf = [0u8; MAX_ENDIAN_BYTES];
        match self {
            Endianness::Little => {
                buf[..bytes.len()].copy_from_slice(bytes);
                u128::from_le_bytes(buf)
            }
            Endianness::Big => {
                let start = MAX_ENDIAN_BYTES - bytes.len();
                buf[start..].copy_from_slice(bytes);
                u128::from_be_bytes(buf)
            }
        }
    }

    #[inline(always)]
    pub fn decode_bits(self, bytes: &[u8], width_bits: usize) -> u128 {
        let value = self.decode_bytes(bytes);
        value & mask_bits(width_bits)
    }

    #[inline(always)]
    pub fn encode_bits(
        self,
        value: u128,
        width_bits: usize,
        byte_len: usize,
    ) -> [u8; MAX_ENDIAN_BYTES] {
        assert!(byte_len <= MAX_ENDIAN_BYTES, "value exceeds 128 bits");
        if byte_len == 0 {
            return [0u8; MAX_ENDIAN_BYTES];
        }
        let masked = value & mask_bits(width_bits);
        let full = match self {
            Endianness::Little => masked.to_le_bytes(),
            Endianness::Big => masked.to_be_bytes(),
        };
        let mut out = [0u8; MAX_ENDIAN_BYTES];
        match self {
            Endianness::Little => {
                out[..byte_len].copy_from_slice(&full[..byte_len]);
            }
            Endianness::Big => {
                let start = MAX_ENDIAN_BYTES - byte_len;
                out[..byte_len].copy_from_slice(&full[start..]);
            }
        }
        out
    }

    #[inline(always)]
    pub fn encode_scalar(self, value: u64, bit_len: u16) -> [u8; MAX_ENDIAN_BYTES] {
        let bits = bit_len as usize;
        let byte_len = ((bits + 7) / 8).max(1);
        self.encode_bits(value as u128, bits, byte_len)
    }
}

#[inline(always)]
pub(crate) fn mask_bits(width_bits: usize) -> u128 {
    if width_bits >= 128 {
        u128::MAX
    } else if width_bits == 0 {
        0
    } else {
        (1u128 << width_bits) - 1
    }
}
