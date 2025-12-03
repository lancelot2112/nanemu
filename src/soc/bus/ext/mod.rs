//! Collection of optional helpers layered on top of `DataHandle` so consumers can opt-in to higher level bus semantics.

pub mod crypto;
pub mod float;
pub mod signed;
pub mod leb128;
pub mod string;
pub mod string_repr;

pub use crypto::CryptoCursorExt;
pub use float::FloatCursorExt;
pub use signed::SignedCursorExt;
pub use leb128::Leb128CursorExt;
pub use string::StringCursorExt;
pub use string_repr::StringReprCursorExt;