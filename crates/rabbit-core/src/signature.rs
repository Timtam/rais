//! Thin wrapper around `rabbit_platform::signature` so callers in `rabbit-core`
//! get a `Result<_, RabbitError>` instead of `std::io::Result`. The verdict
//! type, codesign/signtool dispatch, and tampering classifier all live in
//! `rabbit-platform` — see that crate's docs for the verdict semantics.

use std::path::Path;

pub use rabbit_platform::SignatureVerdict;

use crate::Result;
use crate::error::RabbitError;

pub fn verify_executable_signature(path: &Path) -> Result<SignatureVerdict> {
    rabbit_platform::verify_executable_signature(path).map_err(|source| RabbitError::Io {
        path: path.to_path_buf(),
        source,
    })
}
