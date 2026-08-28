use std::ffi::{CString, c_char, c_void};
use std::io;
use std::path::Path;
use std::ptr::NonNull;

pub mod ffi {
    #[repr(C)]
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub struct Xperm {
        pub low: u16,
        pub high: u16,
        pub reset: bool,
    }
}

pub use ffi::Xperm;

pub const SEPOL_PROC_DOMAIN: &str = "magisk";
pub const SEPOL_FILE_TYPE: &str = "magisk_file";
pub const SEPOL_LOG_TYPE: &str = "magisk_log_file";

mod consts {
    pub use super::{SEPOL_FILE_TYPE, SEPOL_LOG_TYPE, SEPOL_PROC_DOMAIN};
}

mod rules;
mod statement;

pub use statement::format_statement_help;

unsafe extern "C" {
    fn eth_policy_from_file(path: *const c_char) -> *mut c_void;
    fn eth_policy_from_data(data: *const u8, size: usize) -> *mut c_void;
    fn eth_policy_from_split() -> *mut c_void;
    fn eth_policy_compile_split() -> *mut c_void;
    fn eth_policy_free(handle: *mut c_void);
    fn eth_policy_to_file(handle: *const c_void, path: *const c_char) -> bool;

    fn eth_policy_allow(
        handle: *mut c_void,
        source: *const *const c_char,
        source_len: usize,
        target: *const *const c_char,
        target_len: usize,
        class: *const *const c_char,
        class_len: usize,
        perm: *const *const c_char,
        perm_len: usize,
    );
    fn eth_policy_deny(
        handle: *mut c_void,
        source: *const *const c_char,
        source_len: usize,
        target: *const *const c_char,
        target_len: usize,
        class: *const *const c_char,
        class_len: usize,
        perm: *const *const c_char,
        perm_len: usize,
    );
    fn eth_policy_auditallow(
        handle: *mut c_void,
        source: *const *const c_char,
        source_len: usize,
        target: *const *const c_char,
        target_len: usize,
        class: *const *const c_char,
        class_len: usize,
        perm: *const *const c_char,
        perm_len: usize,
    );
    fn eth_policy_dontaudit(
        handle: *mut c_void,
        source: *const *const c_char,
        source_len: usize,
        target: *const *const c_char,
        target_len: usize,
        class: *const *const c_char,
        class_len: usize,
        perm: *const *const c_char,
        perm_len: usize,
    );
    fn eth_policy_xperm(
        handle: *mut c_void,
        action: i32,
        source: *const *const c_char,
        source_len: usize,
        target: *const *const c_char,
        target_len: usize,
        class: *const *const c_char,
        class_len: usize,
        perms: *const Xperm,
        perms_len: usize,
    );
    fn eth_policy_type_state(
        handle: *mut c_void,
        permissive: bool,
        items: *const *const c_char,
        count: usize,
    );
    fn eth_policy_typeattribute(
        handle: *mut c_void,
        types: *const *const c_char,
        types_len: usize,
        attrs: *const *const c_char,
        attrs_len: usize,
    );
    fn eth_policy_type(
        handle: *mut c_void,
        name: *const c_char,
        attrs: *const *const c_char,
        attrs_len: usize,
    );
    fn eth_policy_attribute(handle: *mut c_void, name: *const c_char);
    fn eth_policy_type_rule(
        handle: *mut c_void,
        action: i32,
        source: *const c_char,
        target: *const c_char,
        class: *const c_char,
        default_type: *const c_char,
        object: *const c_char,
    );
    fn eth_policy_genfscon(
        handle: *mut c_void,
        fs_name: *const c_char,
        path: *const c_char,
        context: *const c_char,
    );
    fn eth_policy_print_rules(handle: *const c_void);
}

struct FfiStrings {
    _values: Vec<CString>,
    pointers: Vec<*const c_char>,
}

impl FfiStrings {
    fn new(values: Vec<&str>) -> Option<Self> {
        let values = values
            .into_iter()
            .map(CString::new)
            .collect::<Result<Vec<_>, _>>()
            .ok()?;
        let pointers = values.iter().map(|value| value.as_ptr()).collect();
        Some(Self {
            _values: values,
            pointers,
        })
    }

    fn as_ffi(&self) -> (*const *const c_char, usize) {
        (self.pointers.as_ptr(), self.pointers.len())
    }
}

fn c_string(value: &str) -> Option<CString> {
    CString::new(value).ok()
}

fn path_string(path: &Path) -> io::Result<CString> {
    CString::new(path.to_string_lossy().as_bytes())
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "path contains NUL"))
}

pub struct SePolicy {
    handle: NonNull<c_void>,
}

unsafe impl Send for SePolicy {}

impl Drop for SePolicy {
    fn drop(&mut self) {
        unsafe { eth_policy_free(self.handle.as_ptr()) };
    }
}

impl SePolicy {
    fn from_raw(handle: *mut c_void, message: &'static str) -> io::Result<Self> {
        NonNull::new(handle)
            .map(|handle| Self { handle })
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, message))
    }

    pub fn from_file<P: AsRef<Path>>(path: P) -> io::Result<Self> {
        let path = path_string(path.as_ref())?;
        Self::from_raw(
            unsafe { eth_policy_from_file(path.as_ptr()) },
            "failed to load SELinux policy",
        )
    }

    pub fn from_data(data: &[u8]) -> io::Result<Self> {
        Self::from_raw(
            unsafe { eth_policy_from_data(data.as_ptr(), data.len()) },
            "failed to parse SELinux policy",
        )
    }

    pub fn from_split() -> io::Result<Self> {
        Self::from_raw(
            unsafe { eth_policy_from_split() },
            "failed to load split SELinux policy",
        )
    }

    pub fn compile_split() -> io::Result<Self> {
        Self::from_raw(
            unsafe { eth_policy_compile_split() },
            "failed to compile split SELinux policy",
        )
    }

    pub fn to_file<P: AsRef<Path>>(&self, path: P) -> io::Result<()> {
        let path = path_string(path.as_ref())?;
        if unsafe { eth_policy_to_file(self.handle.as_ptr(), path.as_ptr()) } {
            Ok(())
        } else {
            Err(io::Error::other("failed to write SELinux policy"))
        }
    }

    fn normal_rule(
        &mut self,
        action: i32,
        source: Vec<&str>,
        target: Vec<&str>,
        class: Vec<&str>,
        perm: Vec<&str>,
    ) {
        let (Some(source), Some(target), Some(class), Some(perm)) = (
            FfiStrings::new(source),
            FfiStrings::new(target),
            FfiStrings::new(class),
            FfiStrings::new(perm),
        ) else {
            return;
        };
        let (s, sn) = source.as_ffi();
        let (t, tn) = target.as_ffi();
        let (c, cn) = class.as_ffi();
        let (p, pn) = perm.as_ffi();
        unsafe {
            match action {
                0 => eth_policy_allow(self.handle.as_ptr(), s, sn, t, tn, c, cn, p, pn),
                1 => eth_policy_deny(self.handle.as_ptr(), s, sn, t, tn, c, cn, p, pn),
                2 => eth_policy_auditallow(self.handle.as_ptr(), s, sn, t, tn, c, cn, p, pn),
                _ => eth_policy_dontaudit(self.handle.as_ptr(), s, sn, t, tn, c, cn, p, pn),
            }
        }
    }

    pub fn allow(&mut self, s: Vec<&str>, t: Vec<&str>, c: Vec<&str>, p: Vec<&str>) {
        self.normal_rule(0, s, t, c, p);
    }

    pub fn deny(&mut self, s: Vec<&str>, t: Vec<&str>, c: Vec<&str>, p: Vec<&str>) {
        self.normal_rule(1, s, t, c, p);
    }

    pub fn auditallow(&mut self, s: Vec<&str>, t: Vec<&str>, c: Vec<&str>, p: Vec<&str>) {
        self.normal_rule(2, s, t, c, p);
    }

    pub fn dontaudit(&mut self, s: Vec<&str>, t: Vec<&str>, c: Vec<&str>, p: Vec<&str>) {
        self.normal_rule(3, s, t, c, p);
    }

    fn xperm_rule(
        &mut self,
        action: i32,
        source: Vec<&str>,
        target: Vec<&str>,
        class: Vec<&str>,
        perms: Vec<Xperm>,
    ) {
        let (Some(source), Some(target), Some(class)) = (
            FfiStrings::new(source),
            FfiStrings::new(target),
            FfiStrings::new(class),
        ) else {
            return;
        };
        let (s, sn) = source.as_ffi();
        let (t, tn) = target.as_ffi();
        let (c, cn) = class.as_ffi();
        unsafe {
            eth_policy_xperm(
                self.handle.as_ptr(),
                action,
                s,
                sn,
                t,
                tn,
                c,
                cn,
                perms.as_ptr(),
                perms.len(),
            )
        };
    }

    pub fn allowxperm(&mut self, s: Vec<&str>, t: Vec<&str>, c: Vec<&str>, p: Vec<Xperm>) {
        self.xperm_rule(0, s, t, c, p);
    }

    pub fn auditallowxperm(&mut self, s: Vec<&str>, t: Vec<&str>, c: Vec<&str>, p: Vec<Xperm>) {
        self.xperm_rule(1, s, t, c, p);
    }

    pub fn dontauditxperm(&mut self, s: Vec<&str>, t: Vec<&str>, c: Vec<&str>, p: Vec<Xperm>) {
        self.xperm_rule(2, s, t, c, p);
    }

    fn type_state(&mut self, permissive: bool, types: Vec<&str>) {
        let Some(types) = FfiStrings::new(types) else {
            return;
        };
        let (items, count) = types.as_ffi();
        unsafe { eth_policy_type_state(self.handle.as_ptr(), permissive, items, count) };
    }

    pub fn permissive(&mut self, types: Vec<&str>) {
        self.type_state(true, types);
    }

    pub fn enforce(&mut self, types: Vec<&str>) {
        self.type_state(false, types);
    }

    pub fn typeattribute(&mut self, types: Vec<&str>, attrs: Vec<&str>) {
        let (Some(types), Some(attrs)) = (FfiStrings::new(types), FfiStrings::new(attrs)) else {
            return;
        };
        let (types, types_len) = types.as_ffi();
        let (attrs, attrs_len) = attrs.as_ffi();
        unsafe {
            eth_policy_typeattribute(self.handle.as_ptr(), types, types_len, attrs, attrs_len)
        };
    }

    pub fn type_(&mut self, name: &str, attrs: Vec<&str>) {
        let (Some(name), Some(attrs)) = (c_string(name), FfiStrings::new(attrs)) else {
            return;
        };
        let (attrs, count) = attrs.as_ffi();
        unsafe { eth_policy_type(self.handle.as_ptr(), name.as_ptr(), attrs, count) };
    }

    pub fn attribute(&mut self, name: &str) {
        let Some(name) = c_string(name) else {
            return;
        };
        unsafe { eth_policy_attribute(self.handle.as_ptr(), name.as_ptr()) };
    }

    fn type_rule(
        &mut self,
        action: i32,
        source: &str,
        target: &str,
        class: &str,
        default_type: &str,
        object: &str,
    ) {
        let (Some(source), Some(target), Some(class), Some(default_type), Some(object)) = (
            c_string(source),
            c_string(target),
            c_string(class),
            c_string(default_type),
            c_string(object),
        ) else {
            return;
        };
        unsafe {
            eth_policy_type_rule(
                self.handle.as_ptr(),
                action,
                source.as_ptr(),
                target.as_ptr(),
                class.as_ptr(),
                default_type.as_ptr(),
                object.as_ptr(),
            )
        };
    }

    pub fn type_transition(&mut self, s: &str, t: &str, c: &str, d: &str, o: &str) {
        self.type_rule(0, s, t, c, d, o);
    }

    pub fn type_change(&mut self, s: &str, t: &str, c: &str, d: &str) {
        self.type_rule(1, s, t, c, d, "");
    }

    pub fn type_member(&mut self, s: &str, t: &str, c: &str, d: &str) {
        self.type_rule(2, s, t, c, d, "");
    }

    pub fn genfscon(&mut self, fs_name: &str, path: &str, context: &str) {
        let (Some(fs_name), Some(path), Some(context)) =
            (c_string(fs_name), c_string(path), c_string(context))
        else {
            return;
        };
        unsafe {
            eth_policy_genfscon(
                self.handle.as_ptr(),
                fs_name.as_ptr(),
                path.as_ptr(),
                context.as_ptr(),
            )
        };
    }

    pub fn print_rules(&self) {
        unsafe { eth_policy_print_rules(self.handle.as_ptr()) };
    }
}

#[cfg(all(test, target_os = "linux", not(magiskpolicy_stub)))]
mod tests {
    use super::SePolicy;
    use std::fs;
    use std::path::{Path, PathBuf};
    use std::sync::atomic::{AtomicU64, Ordering};

    static NEXT_TEMP_FILE: AtomicU64 = AtomicU64::new(0);

    struct TempPolicy(PathBuf);

    impl TempPolicy {
        fn new(contents: &[u8]) -> Self {
            let id = NEXT_TEMP_FILE.fetch_add(1, Ordering::Relaxed);
            let path = std::env::temp_dir()
                .join(format!("magiskpolicy-test-{}-{id}.bin", std::process::id()));
            fs::write(&path, contents).expect("write temporary policy");
            Self(path)
        }

        fn path(&self) -> &Path {
            &self.0
        }
    }

    impl Drop for TempPolicy {
        fn drop(&mut self) {
            let _ = fs::remove_file(&self.0);
        }
    }

    #[test]
    fn missing_policy_file_returns_err() {
        let id = NEXT_TEMP_FILE.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "magiskpolicy-missing-{}-{id}.bin",
            std::process::id()
        ));
        let _ = fs::remove_file(&path);

        assert!(SePolicy::from_file(path).is_err());
    }

    #[test]
    fn corrupt_policy_file_returns_err() {
        let policy = TempPolicy::new(b"not a binary SELinux policy");

        assert!(SePolicy::from_file(policy.path()).is_err());
    }

    #[test]
    fn corrupt_policy_data_returns_err() {
        assert!(SePolicy::from_data(b"not a binary SELinux policy").is_err());
    }

    #[test]
    fn missing_split_inputs_return_err() {
        let policy_version = Path::new("/sys/fs/selinux/policyvers");
        let mapping_version = Path::new("/vendor/etc/selinux/plat_sepolicy_vers.txt");
        if policy_version.exists() && mapping_version.exists() {
            return;
        }

        assert!(SePolicy::compile_split().is_err());
    }
}
