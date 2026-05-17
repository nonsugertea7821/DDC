// ddc-core: ContractStore 構築・実効契約計算・検証
// Tech-onRust.md §6 参照

mod effective;
mod error;
mod graph;
mod store;

pub use effective::EffectiveContract;
pub use error::{ValidationError, ValidationErrorKind};
pub use store::ContractStore;
