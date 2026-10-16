//! Deprecated: the keystore moved to [`hatchery_keystore`] (crypto in `tesserax-keytree`).
//! The key tree is not m4a's; m4a is one of its consumers. This crate only re-exports the
//! keystore so 0.2.15 callers keep building. It stays published and is never yanked.
#![deprecated(since = "0.2.16", note = "use hatchery-keystore")]

pub use hatchery_keystore::*;
