//! Parsing of submitted form values into clear "400 Bad Request" messages.

use crate::error::AppError;
use localapp_core::enum_macro::KeyEnum;

/// An optional id (`""` → `None`).
pub fn opt_id(value: &str) -> Option<i64> {
    value.trim().parse().ok()
}

/// A ticked checkbox arrives as any value; an unticked one is absent.
pub fn checked(value: &Option<String>) -> bool {
    value.is_some()
}

/// A `str_enum!` value by key.
pub fn key<T: KeyEnum>(value: &str, what: &str) -> Result<T, AppError> {
    T::from_key(value.trim())
        .ok_or_else(|| AppError::bad_request(format!("Unknown {what} “{value}”.")))
}

/// A required, trimmed, non-empty text value.
pub fn text(value: &str, what: &str) -> Result<String, AppError> {
    let t = value.trim();
    if t.is_empty() {
        return Err(AppError::bad_request(format!("{what} can't be empty.")));
    }
    Ok(t.to_string())
}

pub fn number<T: std::str::FromStr>(value: &str, what: &str) -> Result<T, AppError> {
    value
        .trim()
        .parse()
        .map_err(|_| AppError::bad_request(format!("{what}: “{value}” isn't a number.")))
}
