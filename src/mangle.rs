fn length_prefix(s: &str) -> String {
    format!("{}{}", s.len(), s)
}

pub fn mangle_function(package_name: &str, module_segments: &[&str], func_name: &str) -> String {
    let mut result = String::from("_BG");
    result.push_str(&length_prefix(package_name));
    for seg in module_segments {
        if !seg.is_empty() {
            result.push_str(&length_prefix(seg));
        }
    }
    result.push_str(&length_prefix(func_name));
    result
}

pub fn mangle_method(
    package_name: &str,
    module_segments: &[&str],
    struct_name: &str,
    method_name: &str,
) -> String {
    let mut result = String::from("_BG");
    result.push_str(&length_prefix(package_name));
    for seg in module_segments {
        if !seg.is_empty() {
            result.push_str(&length_prefix(seg));
        }
    }
    result.push_str(&length_prefix(struct_name));
    result.push_str(&length_prefix(method_name));
    result
}

pub fn mangle_entry_main() -> &'static str {
    "main"
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mangle_root_function() {
        let result = mangle_function("my_app", &[""], "add");
        assert_eq!(result, "_BG6my_app3add");
    }

    #[test]
    fn mangle_nested_function() {
        let result = mangle_function("my_app", &["math"], "add");
        assert_eq!(result, "_BG6my_app4math3add");
    }

    #[test]
    fn mangle_deeply_nested() {
        let result = mangle_function("my_app", &["graphics", "renderer"], "draw");
        assert_eq!(result, "_BG6my_app8graphics8renderer4draw");
    }

    #[test]
    fn mangle_method_test() {
        let result = mangle_method("my_app", &["math"], "Vector", "length");
        assert_eq!(result, "_BG6my_app4math6Vector6length");
    }

    #[test]
    fn mangle_with_underscores_no_ambiguity() {
        let a = mangle_function("my_app", &["foo_bar"], "add");
        let b = mangle_function("my_app", &["foo"], "bar_add");
        assert_ne!(a, b);
    }

    #[test]
    fn mangle_main_in_entry_not_mangled() {
        assert_eq!(mangle_entry_main(), "main");
    }
}
