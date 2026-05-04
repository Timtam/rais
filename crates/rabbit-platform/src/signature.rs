//! Platform signature verification for self-update artifacts.
//!
//! `verify_executable_signature` shells out to `/usr/bin/codesign --verify`
//! on macOS and `signtool.exe verify /pa /q` (located via the Windows 10 SDK
//! `bin/<version>/x64/`) on Windows. Three outcomes:
//!
//! - `Signed` — verified valid; proceed.
//! - `Unsigned` — no signature was present, or the host lacks the verification
//!   tooling (e.g. signtool on a typical end-user Windows install). Per
//!   DESIGN.md "where available", this is acceptable: SHA-256 + HTTPS already
//!   chained the artifact to the release manifest.
//! - `Invalid` — the binary claims to be signed but verification surfaced a
//!   clear tampering signal (bad digest, untrusted chain, sealed-resource
//!   mismatch). Callers should reject before touching install state.
//!
//! The classifier is conservative: any non-zero exit is `Unsigned` unless the
//! tool's stderr/stdout matches a curated list of "tampering" keywords. That
//! keeps junk-byte test inputs from false-flagging as `Invalid` while still
//! catching `TRUST_E_BAD_DIGEST`/`cdhash mismatch` in practice.

use std::path::Path;
use std::process::ExitStatus;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum SignatureVerdict {
    Signed { details: String },
    Unsigned { reason: String },
    Invalid { reason: String },
}

impl SignatureVerdict {
    pub fn is_invalid(&self) -> bool {
        matches!(self, SignatureVerdict::Invalid { .. })
    }
}

pub fn verify_executable_signature(path: &Path) -> std::io::Result<SignatureVerdict> {
    verify_executable_signature_impl(path)
}

#[cfg(target_os = "macos")]
fn verify_executable_signature_impl(path: &Path) -> std::io::Result<SignatureVerdict> {
    use std::process::Command;

    let output = Command::new("/usr/bin/codesign")
        .arg("--verify")
        .arg("--strict")
        .arg("--verbose=2")
        .arg(path)
        .output()?;

    let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
    let stderr = String::from_utf8_lossy(&output.stderr).into_owned();
    Ok(verdict_from_codesign_output(
        output.status,
        &stdout,
        &stderr,
    ))
}

#[cfg(target_os = "windows")]
fn verify_executable_signature_impl(path: &Path) -> std::io::Result<SignatureVerdict> {
    use std::process::Command;

    let Some(signtool) = locate_signtool() else {
        return Ok(SignatureVerdict::Unsigned {
            reason: "signtool.exe not found on this host; signature verification skipped"
                .to_string(),
        });
    };

    let output = Command::new(&signtool)
        .arg("verify")
        .arg("/pa")
        .arg("/q")
        .arg(path)
        .output()?;

    let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
    let stderr = String::from_utf8_lossy(&output.stderr).into_owned();
    Ok(verdict_from_signtool_output(
        output.status,
        &stdout,
        &stderr,
    ))
}

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
fn verify_executable_signature_impl(_path: &Path) -> std::io::Result<SignatureVerdict> {
    Ok(SignatureVerdict::Unsigned {
        reason: "platform signature verification is only implemented on Windows and macOS"
            .to_string(),
    })
}

#[cfg(target_os = "windows")]
fn locate_signtool() -> Option<std::path::PathBuf> {
    let program_files_x86 = std::env::var_os("ProgramFiles(x86)")?;
    let sdk_root = std::path::PathBuf::from(program_files_x86)
        .join("Windows Kits")
        .join("10")
        .join("bin");
    let mut candidates: Vec<std::path::PathBuf> = std::fs::read_dir(&sdk_root)
        .ok()?
        .filter_map(|entry| entry.ok())
        .map(|entry| entry.path())
        .filter(|path| path.is_dir())
        .map(|path| path.join("x64").join("signtool.exe"))
        .filter(|path| path.is_file())
        .collect();
    candidates.sort();
    candidates.pop()
}

#[cfg(any(target_os = "macos", test))]
pub(crate) fn verdict_from_codesign_output(
    status: ExitStatus,
    stdout: &str,
    stderr: &str,
) -> SignatureVerdict {
    if status.success() {
        return SignatureVerdict::Signed {
            details: trim_combined(stdout, stderr),
        };
    }
    let combined = trim_combined(stdout, stderr).to_ascii_lowercase();
    if codesign_indicates_tampering(&combined) {
        return SignatureVerdict::Invalid {
            reason: trim_combined(stdout, stderr),
        };
    }
    SignatureVerdict::Unsigned {
        reason: trim_combined(stdout, stderr),
    }
}

#[cfg(any(target_os = "windows", test))]
pub(crate) fn verdict_from_signtool_output(
    status: ExitStatus,
    stdout: &str,
    stderr: &str,
) -> SignatureVerdict {
    if status.success() {
        return SignatureVerdict::Signed {
            details: trim_combined(stdout, stderr),
        };
    }
    let combined = trim_combined(stdout, stderr).to_ascii_lowercase();
    if signtool_indicates_tampering(&combined) {
        return SignatureVerdict::Invalid {
            reason: trim_combined(stdout, stderr),
        };
    }
    SignatureVerdict::Unsigned {
        reason: trim_combined(stdout, stderr),
    }
}

#[cfg(any(target_os = "macos", test))]
fn codesign_indicates_tampering(message: &str) -> bool {
    // Every codesign failure that says "I found a signature, but it does not
    // verify". Anything else (no signature, malformed binary, unreadable) is
    // treated as Unsigned per the "where available" semantics.
    message.contains("a sealed resource is missing or invalid")
        || message.contains("invalid signature")
        || message.contains("hash mismatch")
        || message.contains("signature could not be verified")
        || message.contains("resource fork, finder information, or similar detritus not allowed")
        || message.contains("cdhash mismatch")
}

#[cfg(any(target_os = "windows", test))]
fn signtool_indicates_tampering(message: &str) -> bool {
    // Bad-digest, untrusted-chain, expired-cert: clear "the signature is
    // present but invalid" signals. Anything else (no signature, malformed PE,
    // unreadable) is Unsigned.
    message.contains("0x80096010") // TRUST_E_BAD_DIGEST
        || message.contains("trust_e_bad_digest")
        || message.contains("0x80096002") // TRUST_E_CERT_SIGNATURE
        || message.contains("trust_e_cert_signature")
        || message.contains("0x800b0109") // CERT_E_UNTRUSTEDROOT
        || message.contains("cert_e_untrustedroot")
        || message.contains("certificate chain processed, but terminated")
        || message.contains("the signature is invalid")
        || message.contains("the digital signature of the object did not verify")
}

fn trim_combined(stdout: &str, stderr: &str) -> String {
    let stdout = stdout.trim();
    let stderr = stderr.trim();
    if stdout.is_empty() {
        stderr.to_string()
    } else if stderr.is_empty() {
        stdout.to_string()
    } else {
        format!("{stdout} | {stderr}")
    }
}

#[cfg(test)]
mod tests {
    use std::os::raw::c_int;
    use std::process::ExitStatus;

    use super::{
        SignatureVerdict, codesign_indicates_tampering, signtool_indicates_tampering,
        verdict_from_codesign_output, verdict_from_signtool_output,
    };

    #[cfg(unix)]
    fn exit_status(code: c_int) -> ExitStatus {
        use std::os::unix::process::ExitStatusExt;
        ExitStatus::from_raw((code as i32) << 8)
    }

    #[cfg(windows)]
    fn exit_status(code: c_int) -> ExitStatus {
        use std::os::windows::process::ExitStatusExt;
        ExitStatus::from_raw(code as u32)
    }

    #[test]
    fn classifies_codesign_success_as_signed() {
        let verdict =
            verdict_from_codesign_output(exit_status(0), "", "test-executable: valid on disk");
        assert!(matches!(verdict, SignatureVerdict::Signed { .. }));
    }

    #[test]
    fn classifies_codesign_unsigned_object_as_unsigned() {
        let verdict = verdict_from_codesign_output(
            exit_status(1),
            "",
            "test-executable: code object is not signed at all",
        );
        assert!(matches!(verdict, SignatureVerdict::Unsigned { .. }));
    }

    #[test]
    fn classifies_codesign_non_macho_as_unsigned() {
        let verdict = verdict_from_codesign_output(
            exit_status(1),
            "",
            "/tmp/junk: invalid format for Mach-O file",
        );
        assert!(matches!(verdict, SignatureVerdict::Unsigned { .. }));
    }

    #[test]
    fn classifies_codesign_seal_break_as_invalid() {
        let verdict = verdict_from_codesign_output(
            exit_status(1),
            "",
            "test-executable: a sealed resource is missing or invalid",
        );
        assert!(matches!(verdict, SignatureVerdict::Invalid { .. }));
    }

    #[test]
    fn classifies_signtool_success_as_signed() {
        let verdict =
            verdict_from_signtool_output(exit_status(0), "Successfully verified: RABBIT.exe", "");
        assert!(matches!(verdict, SignatureVerdict::Signed { .. }));
    }

    #[test]
    fn classifies_signtool_no_signature_as_unsigned() {
        let verdict = verdict_from_signtool_output(
            exit_status(1),
            "SignTool Error: No signature was present in the subject.",
            "",
        );
        assert!(matches!(verdict, SignatureVerdict::Unsigned { .. }));
    }

    #[test]
    fn classifies_signtool_chain_failure_as_invalid() {
        let verdict = verdict_from_signtool_output(
            exit_status(1),
            "SignTool Error: A certificate chain processed, but terminated in a root certificate which is not trusted by the trust provider.",
            "",
        );
        assert!(matches!(verdict, SignatureVerdict::Invalid { .. }));
    }

    #[test]
    fn detects_codesign_tampering_phrases() {
        assert!(codesign_indicates_tampering(
            "a sealed resource is missing or invalid"
        ));
        assert!(codesign_indicates_tampering("cdhash mismatch"));
        assert!(!codesign_indicates_tampering(
            "code object is not signed at all"
        ));
        assert!(!codesign_indicates_tampering(
            "/tmp/blob: not in any of the proper formats"
        ));
    }

    #[test]
    fn detects_signtool_tampering_phrases() {
        assert!(signtool_indicates_tampering(
            "winverifytrust returned error: 0x80096010"
        ));
        assert!(signtool_indicates_tampering(
            "a certificate chain processed, but terminated in a root certificate which is not trusted"
        ));
        assert!(!signtool_indicates_tampering(
            "no signature was present in the subject."
        ));
        assert!(!signtool_indicates_tampering(
            "the specified pe header magic value was not found"
        ));
    }
}
