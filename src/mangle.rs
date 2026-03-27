use crate::bir::instruction::BirType;

/// Encode a segment as `<length><name>` (e.g., `3pkg`).
fn length_prefix(s: &str) -> String {
    format!("{}{}", s.len(), s)
}

/// Encode a BirType as a compact string.
///
/// - `i` = Int32, `l` = Int64, `f` = Float32, `d` = Float64
/// - `b` = Bool, `v` = Unit, `e` = Error
/// - `S<len><name>` = struct (e.g., `S5Point`)
/// - `A<type><decimal-size>` = array (e.g., `Ai3` = `[Int32; 3]`)
/// - `T<len><name>` = unresolved TypeParam
fn encode_type(ty: &BirType) -> String {
    match ty {
        BirType::I32 => "i".into(),
        BirType::I64 => "l".into(),
        BirType::F32 => "f".into(),
        BirType::F64 => "d".into(),
        BirType::Bool => "b".into(),
        BirType::Unit => "v".into(),
        BirType::Error => "e".into(),
        BirType::TypeParam(name) => format!("T{}", length_prefix(name)),
        BirType::Struct { name, type_args } => {
            let mut s = format!("S{}", length_prefix(name));
            if !type_args.is_empty() {
                s.push('I');
                for ta in type_args {
                    s.push_str(&encode_type(ta));
                }
                s.push('E');
            }
            s
        }
        BirType::Array { element, size } => {
            format!("A{}{}", encode_type(element), size)
        }
    }
}

/// Build the `N <segments> E` nested-name envelope.
///
/// `extra_names` are appended after `pkg` and `segments`.
fn encode_nested_name(pkg: &str, segments: &[&str], extra_names: &[&str]) -> String {
    let mut s = String::from("N");
    s.push_str(&length_prefix(pkg));
    for seg in segments {
        if !seg.is_empty() {
            s.push_str(&length_prefix(seg));
        }
    }
    for name in extra_names {
        s.push_str(&length_prefix(name));
    }
    s.push('E');
    s
}

/// Append type-args suffix `I...E` if `type_args` is non-empty.
fn encode_type_args(type_args: &[BirType]) -> String {
    if type_args.is_empty() {
        return String::new();
    }
    let mut s = String::from("I");
    for ta in type_args {
        s.push_str(&encode_type(ta));
    }
    s.push('E');
    s
}

/// Mangle a free function.
///
/// Format: `_BGFN<pkg><segments><name>E[I<type-args>E]`
pub fn mangle_function(
    package_name: &str,
    module_segments: &[&str],
    func_name: &str,
    type_args: &[BirType],
) -> String {
    let mut result = String::from("_BGF");
    result.push_str(&encode_nested_name(
        package_name,
        module_segments,
        &[func_name],
    ));
    result.push_str(&encode_type_args(type_args));
    result
}

/// Mangle a method.
///
/// Format: `_BGMN<pkg><segments><struct_name><method>E[I<type-args>E]`
pub fn mangle_method(
    package_name: &str,
    module_segments: &[&str],
    struct_name: &str,
    method_name: &str,
    type_args: &[BirType],
) -> String {
    let mut result = String::from("_BGM");
    result.push_str(&encode_nested_name(
        package_name,
        module_segments,
        &[struct_name, method_name],
    ));
    result.push_str(&encode_type_args(type_args));
    result
}

/// Mangle an initializer.
///
/// Format: `_BGIN<pkg><segments><struct_name>E`
pub fn mangle_initializer(
    package_name: &str,
    module_segments: &[&str],
    struct_name: &str,
) -> String {
    let mut result = String::from("_BGI");
    result.push_str(&encode_nested_name(
        package_name,
        module_segments,
        &[struct_name],
    ));
    result
}

/// The entry-point `main` is never mangled.
pub fn mangle_entry_main() -> &'static str {
    "main"
}

/// Append generic type arguments to an already-mangled base name.
///
/// The base name is expected to end with `E` (the nested-name closing).
/// When `type_args` is non-empty, insert `I<types>E` after the trailing `E`.
///
/// When `type_args` is empty, the base name is returned unchanged.
///
/// # Examples
///
/// ```text
/// base: "_BGFN3pkg8identityE", type_args: [BirType::I32]
/// result: "_BGFN3pkg8identityEIiE"
/// ```
pub fn mangle_generic_suffix(base_mangled: &str, type_args: &[BirType]) -> String {
    if type_args.is_empty() {
        return base_mangled.to_string();
    }
    let mut result = base_mangled.to_string();
    result.push_str(&encode_type_args(type_args));
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mangle_root_function() {
        let result = mangle_function("pkg", &["math"], "add", &[]);
        assert_eq!(result, "_BGFN3pkg4math3addE");
    }

    #[test]
    fn mangle_method_test() {
        let result = mangle_method("pkg", &["math"], "Point", "sum", &[]);
        assert_eq!(result, "_BGMN3pkg4math5Point3sumE");
    }

    #[test]
    fn mangle_initializer_test() {
        let result = mangle_initializer("pkg", &[""], "Point");
        assert_eq!(result, "_BGIN3pkg5PointE");
    }

    #[test]
    fn mangle_generic_function_single() {
        let result = mangle_function("pkg", &[""], "identity", &[BirType::I32]);
        assert_eq!(result, "_BGFN3pkg8identityEIiE");
    }

    #[test]
    fn mangle_generic_function_multi() {
        let result = mangle_function("pkg", &[""], "swap", &[BirType::I32, BirType::Bool]);
        assert_eq!(result, "_BGFN3pkg4swapEIibE");
    }

    #[test]
    fn mangle_generic_struct_method() {
        let type_args = vec![BirType::Struct {
            name: "Point".into(),
            type_args: vec![],
        }];
        let result = mangle_method("pkg", &[""], "Box", "value", &type_args);
        assert_eq!(result, "_BGMN3pkg3Box5valueEIS5PointE");
    }

    #[test]
    fn mangle_array_type_arg() {
        let type_args = vec![BirType::Array {
            element: Box::new(BirType::I32),
            size: 3,
        }];
        let result = mangle_function("pkg", &[""], "foo", &type_args);
        assert_eq!(result, "_BGFN3pkg3fooEIAi3E");
    }

    #[test]
    fn mangle_main_stays_main() {
        assert_eq!(mangle_entry_main(), "main");
    }

    #[test]
    fn no_collision_function_method_initializer() {
        let f = mangle_function("pkg", &[""], "Point", &[]);
        let m = mangle_method("pkg", &[""], "Point", "init", &[]);
        let i = mangle_initializer("pkg", &[""], "Point");
        // All three must differ due to entity tags F, M, I.
        assert_ne!(f, m);
        assert_ne!(f, i);
        assert_ne!(m, i);
    }

    #[test]
    fn mangle_with_underscores_no_ambiguity() {
        let a = mangle_function("my_app", &["foo_bar"], "add", &[]);
        let b = mangle_function("my_app", &["foo"], "bar_add", &[]);
        assert_ne!(a, b);
    }

    #[test]
    fn mangle_generic_suffix_empty() {
        let base = "_BGFN3pkg8identityE";
        let result = mangle_generic_suffix(base, &[]);
        assert_eq!(result, base);
    }

    #[test]
    fn mangle_generic_suffix_with_args() {
        let base = "_BGFN3pkg8identityE";
        let result = mangle_generic_suffix(base, &[BirType::I32]);
        assert_eq!(result, "_BGFN3pkg8identityEIiE");
    }

    #[test]
    fn mangle_deeply_nested() {
        let result = mangle_function("my_app", &["graphics", "renderer"], "draw", &[]);
        assert_eq!(result, "_BGFN6my_app8graphics8renderer4drawE");
    }

    #[test]
    fn encode_type_param() {
        let result = encode_type(&BirType::TypeParam("T".into()));
        assert_eq!(result, "T1T");
    }

    #[test]
    fn encode_error_type() {
        let result = encode_type(&BirType::Error);
        assert_eq!(result, "e");
    }

    #[test]
    fn encode_generic_struct_type() {
        let ty = BirType::Struct {
            name: "Pair".into(),
            type_args: vec![BirType::I32, BirType::Bool],
        };
        let result = encode_type(&ty);
        assert_eq!(result, "S4PairIibE");
    }

    #[test]
    fn mangle_generic_suffix_struct_type_arg() {
        let base = "_BGMN3pkg3Box5valueE";
        let type_args = vec![BirType::Struct {
            name: "Point".into(),
            type_args: vec![],
        }];
        let result = mangle_generic_suffix(base, &type_args);
        assert_eq!(result, "_BGMN3pkg3Box5valueEIS5PointE");
    }
}
