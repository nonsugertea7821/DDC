// ddc-syntax: テキストパーサー・AST 型定義
// Tech-onRust.md §4, §5 参照

mod ast;
mod error;
mod lexer;
mod parser;

pub use ast::*;
pub use error::ParseError;
pub use parser::parse_ddc_file;
