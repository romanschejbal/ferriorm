//! String case-conversion utilities used across the ferriorm workspace.
//!
//! Provides [`to_snake_case`], [`to_pascal_case`], and [`to_camel_case`] for
//! converting between naming conventions (e.g., model names to table names,
//! column names to Rust field names).

/// Convert `PascalCase` or `camelCase` to `snake_case`.
///
/// # Panics
///
/// Panics if a character's lowercase conversion produces no characters,
/// which cannot happen for valid Unicode.
#[must_use]
pub fn to_snake_case(s: &str) -> String {
    let mut result = String::new();
    for (i, c) in s.chars().enumerate() {
        if c.is_uppercase() && i > 0 {
            result.push('_');
        }
        result.push(c.to_lowercase().next().unwrap());
    }
    result
}

/// Convert `snake_case` to `PascalCase`.
///
/// # Panics
///
/// Panics if a character's uppercase conversion produces no characters,
/// which cannot happen for valid Unicode.
#[must_use]
pub fn to_pascal_case(s: &str) -> String {
    let mut result = String::new();
    let mut capitalize_next = true;
    for c in s.chars() {
        if c == '_' {
            capitalize_next = true;
        } else if capitalize_next {
            result.push(c.to_uppercase().next().unwrap());
            capitalize_next = false;
        } else {
            result.push(c);
        }
    }
    result
}

/// Convert `snake_case` to `camelCase`.
#[must_use]
pub fn to_camel_case(s: &str) -> String {
    let pascal = to_pascal_case(s);
    let mut chars = pascal.chars();
    match chars.next() {
        Some(c) => c.to_lowercase().collect::<String>() + chars.as_str(),
        None => String::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_to_snake_case() {
        assert_eq!(to_snake_case("User"), "user");
        assert_eq!(to_snake_case("PostTag"), "post_tag");
        assert_eq!(to_snake_case("createdAt"), "created_at");
        assert_eq!(to_snake_case("HTMLParser"), "h_t_m_l_parser");
    }

    #[test]
    fn test_to_pascal_case() {
        assert_eq!(to_pascal_case("user"), "User");
        assert_eq!(to_pascal_case("post_tag"), "PostTag");
        assert_eq!(to_pascal_case("created_at"), "CreatedAt");
    }

    #[test]
    fn test_to_camel_case() {
        assert_eq!(to_camel_case("user_name"), "userName");
        assert_eq!(to_camel_case("created_at"), "createdAt");
        assert_eq!(to_camel_case("id"), "id");
    }
}
