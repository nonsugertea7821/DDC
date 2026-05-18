// 実効契約型（Tech-onRust.md §6.1 参照）

use std::collections::HashSet;
use ddc_syntax::{FunctionId, StructurePath};

/// 関数の実効契約（宣言契約が Vec であるのに対し、和集合演算のため HashSet を使用）
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct EffectiveContract {
    pub read: HashSet<StructurePath>,
    pub write: HashSet<StructurePath>,
    pub call: HashSet<FunctionId>,
}
