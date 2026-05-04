//! OS-default locale lookup.
//!
//! Returns whatever the platform reports as the current user locale, as a raw
//! string (e.g. `"de-DE"`, `"en-US"`, or POSIX `"de_DE.UTF-8"` on some Unixes).
//! Callers are expected to normalize the result themselves (e.g., strip the
//! charset suffix and translate `_` → `-`) before parsing as BCP-47.
//!
//! Backed by the `sys-locale` crate so we get the same code path on Windows
//! (`GetUserDefaultLocaleName`), macOS (`CFLocaleCopyCurrent`), and Linux
//! (`LC_*` / `LANG`). Returns `None` when the platform reports nothing.

pub fn os_default_locale() -> Option<String> {
    sys_locale::get_locale().filter(|locale| !locale.trim().is_empty())
}
