// シグネチャ変換ユーティリティ（Tech-onRust.md §7, §11.13 参照）
// C# スタイル DDC 型名 → Rust 型名への変換

/// DDC 型名を Rust 型名に変換する
/// - `Optional<T>` → `Option<T>`
/// - `List<T>`     → `Vec<T>`
/// - `void`        → `()`
pub(crate) fn convert_type(ddc_type: &str) -> String {
    // ネストしたジェネリクスに対応するため再帰的に処理する
    if ddc_type == "void" {
        return "()".to_string();
    }
    if let Some(inner) = strip_generic_wrapper(ddc_type, "Optional") {
        return format!("Option<{}>", convert_type(inner));
    }
    if let Some(inner) = strip_generic_wrapper(ddc_type, "List") {
        return format!("Vec<{}>", convert_type(inner));
    }
    ddc_type.to_string()
}

fn strip_generic_wrapper<'a>(s: &'a str, wrapper: &str) -> Option<&'a str> {
    let prefix = format!("{}<", wrapper);
    if s.starts_with(&prefix) && s.ends_with('>') {
        Some(&s[prefix.len()..s.len() - 1])
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_convert_void() {
        assert_eq!(convert_type("void"), "()");
    }

    #[test]
    fn test_convert_optional() {
        assert_eq!(convert_type("Optional<User>"), "Option<User>");
    }

    #[test]
    fn test_convert_list() {
        assert_eq!(convert_type("List<Item>"), "Vec<Item>");
    }

    #[test]
    fn test_convert_nested() {
        assert_eq!(convert_type("Optional<List<Item>>"), "Option<Vec<Item>>");
    }

    #[test]
    fn test_passthrough() {
        assert_eq!(convert_type("UserId"), "UserId");
    }
}
