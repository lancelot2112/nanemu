//! Collection of optional helpers layered on top of `DataHandle` so consumers can opt-in to higher level bus semantics.

pub mod stream;
pub mod string;
pub mod leb128;
pub mod float;
pub mod crypto;
pub mod arbitrary_size;
pub mod string_repr;

pub use crypto::CryptoDataHandleExt;
pub use float::FloatDataHandleExt;
pub use leb128::Leb128DataHandleExt;
pub use arbitrary_size::ArbSizeDataHandleExt;
pub use stream::DataStream;
pub use string::StringDataHandleExt;
pub use string_repr::StringReprDataHandleExt;
