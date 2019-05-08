/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/. */

use std::ffi::CStr;
use std::marker::PhantomData;
use std::os::raw::c_char;

/// `FfiStr<'a>` is a safe (`#[repr(transparent)]`) wrapper around a
/// nul-terminated `*const c_char` (e.g. a C string). Conceptually, it is
/// similar to [`std::ffi::CStr`], except that it may be used in the signatures
/// of extern "C" functions.
///
/// Functions accepting strings should use this instead of accepting a C string
/// directly. This allows us to write those functions using safe code without
/// allowing safe Rust to cause memory unsafety.
///
/// A single function for constructing these from Rust ([`FfiStr::from_raw`])
/// has been provided. Most of the time, this should not be necessary, and users
/// should accept `FfiStr` in the parameter list directly.
///
/// ## Conversions
///
/// Several conversion functions are provided depending on your needs:
///
/// | Function                    | You want         | Handling of null | Handling of invalid utf8 | Notes |
/// | :-------------------------- | :-------         | :--------------- | :----------------------- | :---- |
/// | [`FfiStr::as_str`]          | `&str`           | Panics           | Panics | N/A |
/// | [`FfiStr::as_opt_str`]      | `Option<&str>`   | `None`           | `None` | A warning is logged for invalid utf8, but the warning has no PII |
/// | [`FfiStr::into_string`]     | `String`         | Panics           | Replace with replacement char | Uses `String::from_utf8_lossy` |
/// | [`FfiStr::into_opt_string`] | `Option<String>` | `None`           | Replace with replacement char | Uses `String::from_utf8_lossy` |
/// | [`FfiStr::as_bytes`]        | `&[u8]`          | Panics           | Allowed | Input ends at first nul byte, which is not included |
/// | [`FfiStr::as_opt_bytes`]    | `Option<&[u8]>`  | `None`           | Allowed | Input ends at first nul byte, which is not inclued |
/// | [`FfiStr::as_os_str`]       | `&OsStr`         | Panics           | Allowed on unix, panics on windows | See doc for platform weirdness |
/// | [`FfiStr::as_opt_os_str`]   | `Option<&OsStr>` | `None`           | Allowed on unix, `None` on windows | See doc for platform weirdness |
/// | [`FfiStr::as_path`]         | `&Path`          | Panics           | Allowed on unix, panics on windows | See doc for platform weirdness |
/// | [`FfiStr::as_opt_path`]     | `Option<&Path>`  | `None`           | Allowed on unix, `None` on windows | See doc for platform weirdness |
///
/// ## Caveats
///
/// An effort has been made to make this struct hard to misuse, however it is
/// still possible, if the `'static` lifetime is manually specified in the
/// struct. E.g.
///
/// ```rust,no_run
/// # use ffi_support::FfiStr;
/// // NEVER DO THIS
/// #[no_mangle]
/// extern "C" fn never_do_this(s: FfiStr<'static>) {
///     // save `s` somewhere, and access it after this
///     // function returns.
/// }
/// ```
///
/// Instead, one of the following patterns should be used:
///
/// ```
/// # use ffi_support::FfiStr;
/// #[no_mangle]
/// extern "C" fn valid_use_1(s: FfiStr<'_>) {
///     // Use of `s` after this function returns is impossible
/// }
/// // Alternative:
/// #[no_mangle]
/// extern "C" fn valid_use_2(s: FfiStr) {
///     // Use of `s` after this function returns is impossible
/// }
/// ```
#[repr(transparent)]
pub struct FfiStr<'a> {
    cstr: *const c_char,
    _boo: PhantomData<&'a ()>,
}

impl<'a> FfiStr<'a> {
    /// Construct an `FfiStr` from a raw pointer.
    ///
    /// This should not be needed most of the time, and users should instead
    /// accept `FfiStr` in function parameter lists.
    #[inline]
    pub unsafe fn from_raw(ptr: *const c_char) -> Self {
        Self {
            cstr: ptr,
            _boo: PhantomData,
        }
    }

    /// Construct a FfiStr from a `std::ffi::CStr`. This is provided for
    /// completeness, as a safe method of producing an `FfiStr` in Rust.
    #[inline]
    pub fn from_cstr(cstr: &'a CStr) -> Self {
        Self {
            cstr: cstr.as_ptr(),
            _boo: PhantomData,
        }
    }

    /// Get an `&str` out of the `FfiStr`. This will panic in any case that
    /// [`FfiStr::as_opt_str`] would return `None` (e.g. null pointer or invalid
    /// UTF-8).
    ///
    /// If the string should be optional, you should use [`FfiStr::as_opt_str`]
    /// instead. If an owned string is desired, use [`FfiStr::into_string`] or
    /// [`FfiStr::into_opt_string`].
    #[inline]
    pub fn as_str(&self) -> &'a str {
        self.as_opt_str()
            .expect("Unexpected null string pointer passed to rust")
    }

    /// Get an `Option<&str>` out of the `FfiStr`. If this stores a null
    /// pointer, then None will be returned. If a string containing invalid
    /// UTF-8 was passed, then an error will be logged and `None` will be
    /// returned.
    ///
    /// If the string is a required argument, use [`FfiStr::as_str`], or
    /// [`FfiStr::into_string`] instead. If `Option<String>` is desired, use
    /// [`FfiStr::into_opt_string`] (which will handle invalid UTF-8 by
    /// replacing with the replacement character).
    pub fn as_opt_str(&self) -> Option<&'a str> {
        if self.cstr.is_null() {
            return None;
        }
        unsafe {
            match std::ffi::CStr::from_ptr(self.cstr).to_str() {
                Ok(s) => Some(s),
                Err(e) => {
                    log::error!("Invalid UTF-8 was passed to rust! {:?}", e);
                    None
                }
            }
        }
    }

    /// Get an `Option<String>` out of the `FfiStr`. Returns `None` if this
    /// `FfiStr` holds a null pointer. Note that unlike [`FfiStr::as_opt_str`],
    /// invalid UTF-8 is replaced with the replacement character instead of
    /// causing us to return None.
    ///
    /// If the string should be mandatory, you should use
    /// [`FfiStr::into_string`] instead. If an owned string is not needed, you
    /// may want to use [`FfiStr::as_str`] or [`FfiStr::as_opt_str`] instead,
    /// (however, note the differnces in how invalid UTF-8 is handled, should
    /// this be relevant to your use).
    pub fn into_opt_string(self) -> Option<String> {
        if !self.cstr.is_null() {
            unsafe { Some(CStr::from_ptr(self.cstr).to_string_lossy().to_string()) }
        } else {
            None
        }
    }

    /// Get a `String` out of a `FfiStr`. This function is essential a
    /// convenience wrapper for `ffi_str.into_opt_string().unwrap()`, with a
    /// message that indicates that a null argument was passed to rust when it
    /// should be mandatory. As with [`FfiStr::into_opt_string`], invalid UTF-8
    /// is replaced with the replacement character if encountered.
    ///
    /// If the string should *not* be mandatory, you should use
    /// [`FfiStr::into_opt_string`] instead. If an owned string is not needed,
    /// you may want to use [`FfiStr::as_str`] or [`FfiStr::as_opt_str`]
    /// instead, (however, note the differnces in how invalid UTF-8 is handled,
    /// should this be relevant to your use).
    #[inline]
    pub fn into_string(self) -> String {
        self.into_opt_string()
            .expect("Unexpected null string pointer passed to rust")
    }

    /// Get an `Option<&[u8]>` out of the `FfiStr`. If this stores a null
    /// pointer, then None will be returned. This is similar to `as_str()`,
    /// however it doesn't mind non-utf8 "strings". Input is assumed to end at the
    /// first NUL byte which is *not* included.
    ///
    /// See the "Conversions" section in the [`FfiStr`] documentation for more
    /// info and other functions which may be more useful to you.
    pub fn as_opt_bytes(&self) -> Option<&'a [u8]> {
        if self.cstr.is_null() {
            return None;
        }
        unsafe {
            let end = libc::strlen(self.cstr) as usize;
            Some(std::slice::from_raw_parts(self.cstr as *const u8, end))
        }
    }

    /// Get an `&[u8]` out of the `FfiStr`. If this stores a null pointer, then
    /// we'll panic. This is similar to `as_opt_str()`, however it doesn't mind
    /// non-utf8 "strings". Input is assumed to end at the first NUL byte which
    /// is *not* included.
    ///
    /// See the "Conversions" section in the [`FfiStr`] documentation for more
    /// info and other functions which may be more useful to you.
    pub fn as_bytes(&self) -> &'a [u8] {
        self.as_opt_bytes()
            .expect("Unexpected null pointer passed to rust")
    }

    /// Get an `Option<&OsStr>` out of the `FfiStr`. If this stores a null
    /// pointer, then None will be returned. This is similar to `as_str()`,
    /// however it doesn't mind non-utf8 "strings". Input is assumed to end at
    /// the first NUL byte which is *not* included.
    ///
    /// Note that on windows, this returns None (and warns) if the string is not
    /// valid utf-8. This is not ideal, but is unavoidable at the moment (OsStr
    /// on windows uses WTF-8 encoding, which will basically never come from
    /// anything other than rust itself).
    ///
    /// See the "Conversions" section in the [`FfiStr`] documentation for more
    /// info and other functions which may be more useful to you.
    pub fn as_opt_os_str(&self) -> Option<&'a std::ffi::OsStr> {
        #[cfg(unix)]
        {
            use std::os::unix::ffi::OsStrExt;
            self.as_opt_bytes().map(OsStrExt::from_bytes)
        }
        #[cfg(not(unix))]
        {
            self.as_opt_str().map(std::ffi::OsStr::from)
        }
    }

    /// Get an `Option<&OsStr>` out of the `FfiStr`. If this stores a null
    /// pointer, then None will be returned. This is similar to `as_str()`,
    /// however it doesn't mind non-utf8 "strings". Input is assumed to end at
    /// the first NUL byte which is *not* included.
    ///
    /// Note that on windows, this returns panics if the string is not valid
    /// utf-8. This is not ideal, but is unavoidable at the moment (OsStr on
    /// windows uses WTF-8 encoding, which will basically never come from
    /// anything other than rust itself).
    ///
    /// See the "Conversions" section in the [`FfiStr`] documentation for more
    /// info and other functions which may be more useful to you.
    pub fn as_os_str(&self) -> &'a std::ffi::OsStr {
        self.as_opt_os_str()
            .expect("Unexpected null pointer or invalid string passed to rust")
    }

    /// Get an `Option<&OsStr>` out of the `FfiStr`. If this stores a null
    /// pointer, then None will be returned. This is similar to `as_str()`,
    /// however it doesn't mind non-utf8 "strings". Input is assumed to end at
    /// the first NUL byte which is *not* included.
    ///
    /// Note that on windows, this returns None (and warns) if the string is not
    /// valid utf-8. This is not ideal, but is unavoidable at the moment (OsStr
    /// on windows uses WTF-8 encoding, which will basically never come from
    /// anything other than rust itself).
    ///
    /// See the "Conversions" section in the [`FfiStr`] documentation for more
    /// info and other functions which may be more useful to you.
    pub fn as_opt_path(&self) -> Option<&'a std::path::Path> {
        self.as_opt_os_str().map(std::path::Path::new)
    }

    /// Get an `Option<&OsStr>` out of the `FfiStr`. If this stores a null
    /// pointer, then None will be returned. This is similar to `as_str()`,
    /// however it doesn't mind non-utf8 "strings". Input is assumed to end at
    /// the first NUL byte which is *not* included.
    ///
    /// Note that on windows, this returns None (and warns) if the string is not
    /// valid utf-8. This is not ideal, but is unavoidable at the moment (OsStr
    /// on windows uses WTF-8 encoding, which will basically never come from
    /// anything other than rust itself).
    ///
    /// See the "Conversions" section in the [`FfiStr`] documentation for more
    /// info and other functions which may be more useful to you.
    pub fn as_path(&self) -> &'a std::path::Path {
        std::path::Path::new(self.as_os_str())
    }
}

impl<'a> std::fmt::Debug for FfiStr<'a> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if let Some(s) = self.as_opt_str() {
            write!(f, "FfiStr({:?})", s)
        } else {
            write!(f, "FfiStr(null)")
        }
    }
}

// Conversions...

impl<'a> From<FfiStr<'a>> for String {
    #[inline]
    fn from(f: FfiStr<'a>) -> Self {
        f.into_string()
    }
}

impl<'a> From<FfiStr<'a>> for Option<String> {
    #[inline]
    fn from(f: FfiStr<'a>) -> Self {
        f.into_opt_string()
    }
}

impl<'a> From<FfiStr<'a>> for Option<&'a str> {
    #[inline]
    fn from(f: FfiStr<'a>) -> Self {
        f.as_opt_str()
    }
}

impl<'a> From<FfiStr<'a>> for &'a str {
    #[inline]
    fn from(f: FfiStr<'a>) -> Self {
        f.as_str()
    }
}

impl<'a> From<FfiStr<'a>> for &'a std::path::Path {
    fn from(f: FfiStr<'a>) -> Self {
        f.as_path()
    }
}

impl<'a> From<FfiStr<'a>> for Option<&'a std::path::Path> {
    fn from(f: FfiStr<'a>) -> Self {
        f.as_opt_path()
    }
}

impl<'a> From<FfiStr<'a>> for &'a std::ffi::OsStr {
    fn from(f: FfiStr<'a>) -> Self {
        f.as_os_str()
    }
}

impl<'a> From<FfiStr<'a>> for Option<&'a std::ffi::OsStr> {
    fn from(f: FfiStr<'a>) -> Self {
        f.as_opt_os_str()
    }
}

impl<'a> From<FfiStr<'a>> for std::ffi::OsString {
    fn from(f: FfiStr<'a>) -> Self {
        f.as_os_str().to_owned()
    }
}

impl<'a> From<FfiStr<'a>> for Option<std::ffi::OsString> {
    fn from(f: FfiStr<'a>) -> Self {
        f.as_opt_os_str().map(ToOwned::to_owned)
    }
}

impl<'a> From<FfiStr<'a>> for std::path::PathBuf {
    fn from(f: FfiStr<'a>) -> Self {
        f.as_path().to_owned()
    }
}

impl<'a> From<FfiStr<'a>> for Option<std::path::PathBuf> {
    fn from(f: FfiStr<'a>) -> Self {
        f.as_opt_path().map(ToOwned::to_owned)
    }
}

impl<'a> From<FfiStr<'a>> for Option<&'a [u8]> {
    fn from(f: FfiStr<'a>) -> Self {
        f.as_opt_bytes()
    }
}

impl<'a> From<FfiStr<'a>> for &'a [u8] {
    fn from(f: FfiStr<'a>) -> Self {
        f.as_bytes()
    }
}

impl<'a> From<FfiStr<'a>> for Option<Vec<u8>> {
    fn from(f: FfiStr<'a>) -> Self {
        f.as_opt_bytes().map(ToOwned::to_owned)
    }
}

impl<'a> From<FfiStr<'a>> for Vec<u8> {
    fn from(f: FfiStr<'a>) -> Self {
        f.as_bytes().to_owned()
    }
}

impl<'a> AsRef<str> for FfiStr<'a> {
    fn as_ref(&self) -> &str {
        self.as_str()
    }
}

impl<'a> AsRef<[u8]> for FfiStr<'a> {
    fn as_ref(&self) -> &[u8] {
        self.as_bytes()
    }
}

impl<'a> AsRef<std::ffi::OsStr> for FfiStr<'a> {
    fn as_ref(&self) -> &std::ffi::OsStr {
        self.as_os_str()
    }
}

impl<'a> AsRef<std::path::Path> for FfiStr<'a> {
    fn as_ref(&self) -> &std::path::Path {
        self.as_path()
    }
}

// Comparisons...

// Compare FfiStr with eachother
impl<'a> PartialEq for FfiStr<'a> {
    #[inline]
    fn eq(&self, other: &FfiStr<'a>) -> bool {
        self.as_opt_str() == other.as_opt_str()
    }
}

// Compare FfiStr with str
impl<'a> PartialEq<str> for FfiStr<'a> {
    #[inline]
    fn eq(&self, other: &str) -> bool {
        self.as_opt_str() == Some(other)
    }
}

// Compare FfiStr with &str
impl<'a, 'b> PartialEq<&'b str> for FfiStr<'a> {
    #[inline]
    fn eq(&self, other: &&'b str) -> bool {
        self.as_opt_str() == Some(*other)
    }
}

// rhs/lhs swap version of above
impl<'a> PartialEq<FfiStr<'a>> for str {
    #[inline]
    fn eq(&self, other: &FfiStr<'a>) -> bool {
        Some(self) == other.as_opt_str()
    }
}

// rhs/lhs swap...
impl<'a, 'b> PartialEq<FfiStr<'a>> for &'b str {
    #[inline]
    fn eq(&self, other: &FfiStr<'a>) -> bool {
        Some(*self) == other.as_opt_str()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn test_ffi_str_basic() {
        let ffis = unsafe { FfiStr::from_raw(b"abcdef\0".as_ptr() as *const _) };
        let ffis_null = unsafe { FfiStr::from_raw(std::ptr::null()) };
        let ffis_bad_utf8 = unsafe { FfiStr::from_raw(b"abcde\xff1234\0".as_ptr() as *const _) };

        assert_eq!(ffis.as_str(), "abcdef");
        assert_eq!(ffis.as_bytes(), b"abcdef");

        assert!(ffis.as_opt_str().is_some());
        assert!(ffis.as_opt_bytes().is_some());

        assert!(ffis.as_opt_str().is_some());
        assert!(ffis.as_opt_bytes().is_some());

        assert!(ffis_null.as_opt_str().is_none());
        assert!(ffis_null.as_opt_bytes().is_none());

        assert!(ffis_bad_utf8.as_opt_str().is_none());

        assert_eq!(
            ffis_bad_utf8.as_opt_bytes(),
            Some(b"abcde\xff1234".as_ref())
        );
        assert_eq!(ffis_bad_utf8.into_string(), "abcde\u{FFFD}1234");
    }
}
