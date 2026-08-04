/// Returns `true` for use with `serde(default = ...)` attributes.
pub const fn default_true() -> bool {
    true
}

/// Returns whether `value` equals its type's default value.
pub fn is_default<T: Default + PartialEq>(value: &T) -> bool {
    *value == T::default()
}
