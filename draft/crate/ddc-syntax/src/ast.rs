// AST 型定義（Tech-onRust.md §4 参照）

// ─── §4.1 基本識別子 ────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct StructurePath(pub String);

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct FunctionId(pub String);

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct TypeId(pub String);

/// `Read:` セクションの 1 エントリ（§4.1）
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReadPath {
    pub path: StructurePath,
    /// `as x` 宣言が存在する場合 `Some("x")`、なければ `None`
    pub alias: Option<String>,
}

// ─── §4.2 宣言契約 ──────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct DeclaredContract {
    pub read: Vec<ReadPath>,
    pub write: Vec<StructurePath>,
    pub call: Vec<FunctionId>,
}

// ─── §4.3 DDC アノテーション ────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DdcAnnotation {
    Kernel,
}

// ─── §4.4 DDC Body ──────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DdcBody {
    Rust(String),
}

// ─── §4.5 DDC 関数 ──────────────────────────────────────────────────────────

/// 関数の 1 パラメータ（C# スタイル: `型名 引数名`）
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Param {
    pub type_name: String,
    pub param_name: String,
}

/// DDC 関数のシグネチャ（構造化）
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DdcFnSig {
    /// "void", "Optional<User>", "List<Item>" など
    pub return_type: String,
    pub name: FunctionId,
    pub params: Vec<Param>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DdcFn {
    pub annotations: Vec<DdcAnnotation>,
    pub id: FunctionId,
    pub is_readonly: bool,
    /// `true` = 契約ブロック `( ... )` が構文上存在した。`false` = 省略された（§2.4 違反）。
    pub has_contract_block: bool,
    pub sig: DdcFnSig,
    pub contract: DeclaredContract,
    pub body: DdcBody,
}

// ─── §4.6 contract / impl 宣言 ──────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContractMethod {
    pub id: FunctionId,
    pub sig: DdcFnSig,
    pub contract: DeclaredContract,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContractDecl {
    pub id: TypeId,
    pub envelope: DeclaredContract,
    pub methods: Vec<ContractMethod>,
}

/// class 内メソッドの契約の出所を示す。
/// `ContractImpl` は contract メソッドへの委譲、`OwnContract` は自身の契約ブロック。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ImplMethodKind {
    /// `: C1.m` または `: C1.m, C2.m` — 契約を contract メソッドに委譲する。
    /// 複数 contract の同名メソッドを 1 関数で満たす場合はコンマで列挙（§11.18）。
    ContractImpl { contract_refs: Vec<FunctionId> },
    /// `( Read: ... )` — class メソッドが自身の契約ブロックを持つ。
    /// contract に対応しない private helper 等に使用する。
    OwnContract { contract: DeclaredContract },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImplMethod {
    pub id:   FunctionId,
    pub sig:  DdcFnSig,
    pub kind: ImplMethodKind,
    pub body: DdcBody,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImplDecl {
    pub type_id:      TypeId,
    pub contract_ids: Vec<TypeId>,  // 実装する contract のリスト（1つ以上）
    pub methods:      Vec<ImplMethod>,
}

// ─── §4.7 ファイルトップレベル ──────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DdcItem {
    RustPassthrough(String),
    DdcFn(DdcFn),
    Contract(ContractDecl),
    Impl(ImplDecl),
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct DdcFile {
    pub items: Vec<DdcItem>,
}
