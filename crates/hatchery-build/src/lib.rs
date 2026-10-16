//! Remote build and sign for hatchery nodes.
//!
//! A node that must not compile locally and must not hold signing secrets asks C2
//! (`POST /v1/build` on the enrollment listener, signed with its identity key). C2 checks the
//! node's status and the `build` capability, then asks the build host, which checks admission
//! again, runs a configured recipe, hashes the artifact and signs a receipt with a key that
//! never leaves the host.

#[cfg(unix)]
pub mod host;
pub mod http;
pub mod wire;

#[cfg(unix)]
pub use host::{serve, Admission, BuildError, BuildHost, C2Admission, Recipe, Recipes, Signer};
pub use wire::{BuildReceipt, BuildResponse, HostRequest, ReceiptError, SignedBuildRequest};
