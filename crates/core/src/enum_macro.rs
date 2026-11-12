//! A small macro for "string-keyed" enums.
//!
//! Many enums (roles, themes, statuses, ...) need
//! the same plumbing: a stable storage key, a human label for the UI, a list of
//! all variants for combo boxes, and parsing back from the key. This macro
//! generates all of that so each enum is declared in one place.

/// Implemented by every enum declared with [`str_enum!`].
pub trait KeyEnum: Sized + Copy + 'static {
    /// Every variant, in declaration order.
    fn all() -> &'static [Self];
    /// Stable storage key (used in the database and settings files).
    fn key(self) -> &'static str;
    /// Human readable label for the UI.
    fn label(self) -> &'static str;
    /// Parse a storage key.
    fn from_key(key: &str) -> Option<Self>;
}

/// Declare an enum whose variants map to a storage key and a UI label.
///
/// ```ignore
/// str_enum! {
///     /// Docs for the enum.
///     pub enum Colour { Red => ("red", "Red"), Blue => ("blue", "Blue") }
/// }
/// ```
#[macro_export]
macro_rules! str_enum {
    (
        $(#[$meta:meta])*
        pub enum $name:ident { $($variant:ident => ($key:literal, $label:literal)),+ $(,)? }
    ) => {
        $(#[$meta])*
        #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord,
                 serde::Serialize, serde::Deserialize)]
        pub enum $name { $($variant),+ }

        impl $name {
            /// Every variant, in declaration order.
            pub const ALL: &'static [$name] = &[$($name::$variant),+];
            /// Stable storage key.
            pub fn key(self) -> &'static str { match self { $($name::$variant => $key),+ } }
            /// Human readable label.
            pub fn label(self) -> &'static str { match self { $($name::$variant => $label),+ } }
            /// Parse a storage key.
            pub fn from_key(key: &str) -> Option<Self> {
                match key { $($key => Some($name::$variant),)+ _ => None }
            }
        }

        impl $crate::enum_macro::KeyEnum for $name {
            fn all() -> &'static [Self] { Self::ALL }
            fn key(self) -> &'static str { $name::key(self) }
            fn label(self) -> &'static str { $name::label(self) }
            fn from_key(key: &str) -> Option<Self> { $name::from_key(key) }
        }

        impl std::fmt::Display for $name {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                f.write_str(self.label())
            }
        }
    };
}
