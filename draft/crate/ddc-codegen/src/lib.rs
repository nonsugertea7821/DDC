// ddc-codegen: DdcFile AST → Rust TokenStream 生成
// Tech-onRust.md §7 参照

mod error;
mod gen;
mod sig;

pub use error::CodegenError;
pub use gen::codegen;
