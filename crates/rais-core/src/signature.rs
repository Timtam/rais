//! Thin wrapper around `rais_platform::signature` so callers in `rais-core`
//! get a `Result<_, RaisError>` instead of `std::io::Result`. The verdict
//! type, codesign/signtool dispatch, and tampering classifier all live in
//! `rais-platform` — see that crate's docs for the verdict semantics.

use std::path::Path;

pub use rais_platform::SignatureVerdict;

use crate::Result;
use crate::error::RaisError;

pub fn verify_executable_signature(path: &Path) -> Result<SignatureVerdict> {
    rais_platform::verify_executable_signature(path).map_err(|source| RaisError::Io {
        path: path.to_path_buf(),
        source,
    })
}
