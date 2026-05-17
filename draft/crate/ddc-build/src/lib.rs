// ddc-build: build.rs 向けエントリーポイント
// Tech-onRust.md §8 参照

mod analyzer;
mod driver;

pub use driver::compile;
