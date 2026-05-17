// ビルドドライバー（Tech-onRust.md §8.2 参照）

use std::{fs, path::{Path, PathBuf}};
use ddc_syntax::{DdcFile, DdcItem, parse_ddc_file};
use ddc_core::ContractStore;
use ddc_codegen::codegen;
use crate::analyzer::BodyAnalyzer;

/// DDC ビルドエントリーポイント。`build.rs` から呼び出す。
///
/// 処理フロー（Tech-onRust.md §8.2）:
/// 1. src_dir 以下の *.ddc ファイルを再帰的に走査
/// 2. cargo:rerun-if-changed で変更監視を登録
/// 3. 各ファイルを parse_ddc_file() でパース
/// 4. ContractStore::from_files() で ContractStore 構築
/// 5. validate_readonly() / validate_kernel_boundary() で検証
/// 6. BodyAnalyzer::validate() で body の DDC 適合性を検証
/// 7. ddc_codegen::codegen() で TokenStream 生成
/// 8. $OUT_DIR/ddc_generated.rs に書き出し
///
/// # Example
/// ```no_run
/// fn main() {
///     ddc_build::compile("src/");
/// }
/// ```
pub fn compile<P: AsRef<Path>>(src_dir: P) {
    let src_dir = src_dir.as_ref();

    // ① *.ddc ファイル収集
    let ddc_paths = collect_ddc_files(src_dir);

    // ② cargo 変更監視登録
    println!("cargo:rerun-if-changed={}", src_dir.display());
    for path in &ddc_paths {
        println!("cargo:rerun-if-changed={}", path.display());
    }

    // ③ パース
    let files: Vec<DdcFile> = ddc_paths.iter().map(|path| {
        let text = fs::read_to_string(path)
            .unwrap_or_else(|e| panic!("DDC read error {}: {e}", path.display()));
        parse_ddc_file(&text)
            .unwrap_or_else(|e| panic!("DDC parse error {}: {e:?}", path.display()))
    }).collect();

    // ④ ContractStore 構築
    let store = ContractStore::from_files(files.iter());

    // ⑤ 宣言契約レベル検証
    let mut errors = store.validate_readonly();
    errors.extend(store.validate_kernel_boundary());

    // ⑥ body レベル検証（DdcFn のみ; ImplMethod は将来対応）
    for file in &files {
        for item in &file.items {
            if let DdcItem::DdcFn(f) = item {                // 契約ブロック必須化（§2.4）
                if !f.has_contract_block {
                    errors.push(ddc_core::ValidationError {
                        kind:     ddc_core::ValidationErrorKind::MissingContractBlock,
                        function: f.id.clone(),
                        message:  format!("関数 '{}' に契約ブロック '()' がありません", f.id.0),
                    });
                }                errors.extend(BodyAnalyzer::validate(f));
            }
        }
    }

    if !errors.is_empty() {
        for e in &errors {
            eprintln!("DDC error [{}] in '{}': {}", format!("{:?}", e.kind), e.function.0, e.message);
        }
        panic!("DDC validation failed: {} error(s)", errors.len());
    }

    // ⑦ コード生成
    let mut out_tokens = proc_macro2::TokenStream::new();
    for file in &files {
        let ts = codegen(file)
            .unwrap_or_else(|e| panic!("DDC codegen error: {e}"));
        out_tokens.extend(ts);
    }

    // ⑧ 書き出し
    let out_dir = std::env::var("OUT_DIR")
        .unwrap_or_else(|_| panic!("OUT_DIR not set (compile() must be called from build.rs)"));
    let out_path = PathBuf::from(out_dir).join("ddc_generated.rs");
    fs::write(&out_path, out_tokens.to_string())
        .unwrap_or_else(|e| panic!("DDC write error {}: {e}", out_path.display()));
}

/// `dir` 以下の `*.ddc` ファイルを再帰的に収集する
fn collect_ddc_files(dir: &Path) -> Vec<PathBuf> {
    let mut files = Vec::new();
    if !dir.is_dir() {
        return files;
    }
    let rd = match fs::read_dir(dir) {
        Ok(rd) => rd,
        Err(e) => {
            eprintln!("DDC: cannot read dir {}: {e}", dir.display());
            return files;
        }
    };
    for entry in rd.flatten() {
        let path = entry.path();
        if path.is_dir() {
            files.extend(collect_ddc_files(&path));
        } else if path.extension().map(|e| e == "ddc").unwrap_or(false) {
            files.push(path);
        }
    }
    files
}

