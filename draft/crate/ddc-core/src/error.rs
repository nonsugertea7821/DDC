// 検証エラー型（Tech-onRust.md §6.4 参照）

use ddc_syntax::FunctionId;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ValidationError {
    pub kind: ValidationErrorKind,
    pub function: FunctionId,
    pub message: String,
}

impl std::fmt::Display for ValidationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "[{:?}] {}: {}", self.kind, self.function.0, self.message)
    }
}

impl std::error::Error for ValidationError {}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ValidationErrorKind {
    // 宣言契約レベルの検証（ContractStore）
    ReadonlyViolation,
    KernelBoundaryViolation,

    // body レベルの検証（BodyAnalyzer）
    BodyUndeclaredRead,
    BodyUndeclaredWrite,
    BodyUndeclaredCall,
    BodyUndeclaredThrow,
    BodyUnsafeBlock,
    BodyForbiddenMacro,

    // エイリアス宣言レベルの検証
    AliasNameConflict,
    AliasOnNonReadSection,

    // 構造レベルの検証
    /// DDC 関数に契約ブロック `( ... )` が存在しない（§2.4）
    MissingContractBlock,
}
