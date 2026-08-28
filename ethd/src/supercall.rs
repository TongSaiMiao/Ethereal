use std::{
    ffi::{CStr, CString},
    fs::File,
    io::{self, Read},
    mem::size_of,
    os::raw::c_int,
    process,
    sync::{
        Arc, Mutex,
        atomic::{AtomicI32, Ordering},
    },
};

use libc::{EINVAL, c_long, c_void, uid_t};
use log::{error, info, warn};

use crate::package::{read_package_config, synchronize_package_uid};

const KSTORAGE_EXCLUDE_LIST_GROUP: i32 = 1;

const SUPERCALL_SU: c_long = 0x1010;
const SUPERCALL_KSTORAGE_WRITE: c_long = 0x1041;
const SUPERCALL_SU_GRANT_UID: c_long = 0x1100;
const SUPERCALL_SU_REVOKE_UID: c_long = 0x1101;
const SUPERCALL_SU_NUMS: c_long = 0x1102;
const SUPERCALL_SU_LIST: c_long = 0x1103;
const SUPERCALL_SU_RESET_PATH: c_long = 0x1111;
const SUPERCALL_SU_GET_SAFEMODE: c_long = 0x1112;

const SUPERCALL_SCONTEXT_LEN: usize = 0x60;
const SUPERCALL_HELLO_MAGIC: u32 = 0x1158_1158;
const ETHEREAL_MAGIC2: u32 = 0x4554_4852;
const ETHEREAL_IOCTL: libc::Ioctl = 0x4554_4801;
const SYS_REBOOT: c_long = 142;
const MANAGER_TOKEN_SIZE: usize = 32;
static ZERO_MANAGER_TOKEN: [u8; MANAGER_TOKEN_SIZE] = [0; MANAGER_TOKEN_SIZE];

#[repr(C)]
struct EtherealSc {
    magic: u32,
    cmd: u32,
    a2: u64,
    a3: u64,
    a4: u64,
    ret: i64,
    token: [u8; MANAGER_TOKEN_SIZE],
}

fn ethereal_fd() -> c_int {
    static FD: AtomicI32 = AtomicI32::new(-1);
    let cur = FD.load(Ordering::Relaxed);
    if cur >= 0 {
        return cur;
    }
    let mut got: c_int = -1;
    unsafe {
        libc::syscall(
            SYS_REBOOT,
            SUPERCALL_HELLO_MAGIC as c_long,
            ETHEREAL_MAGIC2 as c_long,
            ZERO_MANAGER_TOKEN.as_ptr() as c_long,
            &mut got as *mut c_int,
        );
    }
    if got >= 0 {
        FD.store(got, Ordering::Relaxed);
    }
    got
}

fn ethereal_call(cmd: c_long, a2: u64, a3: u64, a4: u64) -> c_long {
    let fd = ethereal_fd();
    if fd < 0 {
        return -(libc::ENOSYS as c_long);
    }
    let mut sc = EtherealSc {
        magic: SUPERCALL_HELLO_MAGIC,
        cmd: (cmd & 0xFFFF) as u32,
        a2,
        a3,
        a4,
        ret: 0,
        token: [0; MANAGER_TOKEN_SIZE],
    };
    let rc = unsafe { libc::ioctl(fd, ETHEREAL_IOCTL, &mut sc as *mut EtherealSc) };
    if rc < 0 {
        return -(io::Error::last_os_error()
            .raw_os_error()
            .unwrap_or(libc::EIO) as c_long);
    }
    sc.ret as c_long
}

#[repr(C)]
struct SuProfile {
    uid: i32,
    to_uid: i32,
    scontext: [u8; SUPERCALL_SCONTEXT_LEN],
}

fn sc_su_revoke_uid(uid: uid_t) -> c_long {
    ethereal_call(SUPERCALL_SU_REVOKE_UID, uid as u64, 0, 0)
}

fn sc_su_grant_uid(profile: &SuProfile) -> c_long {
    ethereal_call(
        SUPERCALL_SU_GRANT_UID,
        profile as *const SuProfile as u64,
        0,
        0,
    )
}

fn sc_kstorage_write(gid: i32, did: i64, _data: *mut c_void, _offset: i32, _dlen: i32) -> c_long {
    ethereal_call(SUPERCALL_KSTORAGE_WRITE, gid as u64, did as u64, 0)
}

fn sc_set_module_exclude(uid: i64, exclude: i32) -> c_long {
    sc_kstorage_write(
        KSTORAGE_EXCLUDE_LIST_GROUP,
        uid,
        &exclude as *const i32 as *mut c_void,
        0,
        size_of::<i32>() as i32,
    )
}

pub fn sc_su_get_safemode() -> c_long {
    ethereal_call(SUPERCALL_SU_GET_SAFEMODE, 0, 0, 0)
}

fn sc_su(profile: &SuProfile) -> c_long {
    ethereal_call(SUPERCALL_SU, profile as *const SuProfile as u64, 0, 0)
}

fn sc_su_reset_path(path: &CStr) -> c_long {
    if path.to_bytes().is_empty() {
        return (-EINVAL).into();
    }
    ethereal_call(SUPERCALL_SU_RESET_PATH, path.as_ptr() as u64, 0, 0)
}

fn sc_su_uid_nums() -> c_long {
    ethereal_call(SUPERCALL_SU_NUMS, 0, 0, 0)
}

fn sc_su_allow_uids(buf: &mut [uid_t]) -> c_long {
    if buf.is_empty() {
        return (-EINVAL).into();
    }
    ethereal_call(
        SUPERCALL_SU_LIST,
        buf.as_mut_ptr() as u64,
        buf.len() as u64,
        0,
    )
}

fn read_file_to_string(path: &str) -> io::Result<String> {
    let mut file = File::open(path)?;
    let mut content = String::new();
    file.read_to_string(&mut content)?;
    Ok(content)
}

fn convert_string_to_u8_array(s: &str) -> [u8; SUPERCALL_SCONTEXT_LEN] {
    let mut u8_array = [0u8; SUPERCALL_SCONTEXT_LEN];
    let bytes = s.as_bytes();
    let len = usize::min(SUPERCALL_SCONTEXT_LEN, bytes.len());
    u8_array[..len].copy_from_slice(&bytes[..len]);
    u8_array
}

pub fn refresh_package_list(mutex: &Arc<Mutex<()>>) {
    let _lock = mutex.lock().unwrap();

    let num = sc_su_uid_nums();
    if num < 0 {
        error!("[refresh_su_list] Error getting number of UIDs: {}", num);
        return;
    }
    let num = num as usize;
    let mut uids = vec![0 as uid_t; num];
    let n = sc_su_allow_uids(&mut uids);
    if n < 0 {
        error!("[refresh_su_list] Error getting su list");
        return;
    }
    for uid in &uids {
        if *uid == 0 || *uid == 2000 {
            warn!("[refresh_package_list] Skip revoking critical uid: {}", uid);
            continue;
        }
        info!("[refresh_package_list] Revoking {} root permission...", uid);
        let rc = sc_su_revoke_uid(*uid);
        if rc != 0 {
            error!("[refresh_package_list] Error revoking UID: {}", rc);
        }
    }

    if let Err(e) = synchronize_package_uid() {
        error!("Failed to synchronize package UIDs: {}", e);
    }

    let package_configs = read_package_config();
    for config in package_configs {
        if config.allow == 1 && config.exclude == 0 {
            let profile = SuProfile {
                uid: config.uid,
                to_uid: config.to_uid,
                scontext: convert_string_to_u8_array(&config.sctx),
            };
            let result = sc_su_grant_uid(&profile);
            info!(
                "[refresh_package_list] Loading {}: result = {}",
                config.pkg, result
            );
        }
        if config.allow == 0 && config.exclude == 1 {
            let result = sc_set_module_exclude(config.uid as i64, 1);
            info!(
                "[refresh_package_list] Loading exclude {}: result = {}",
                config.pkg, result
            );
        }
    }
}

pub fn privilege_ethd_profile() {
    let all_allow_ctx = "u:r:magisk:s0";
    let profile = SuProfile {
        uid: process::id().try_into().expect("PID conversion failed"),
        to_uid: 0,
        scontext: convert_string_to_u8_array(all_allow_ctx),
    };
    let result = sc_su(&profile);
    info!("[privilege_ethd_profile] result = {}", result);
}

pub fn init_load_su_path() {
    let su_path_file = "/data/adb/eth/su_path";

    match read_file_to_string(su_path_file) {
        Ok(su_path) => match CString::new(su_path.trim()) {
            Ok(su_path_cstr) => {
                let result = sc_su_reset_path(&su_path_cstr);
                if result == 0 {
                    info!("suPath load successfully");
                } else {
                    warn!("Failed to load su path, error code: {}", result);
                }
            }
            Err(e) => {
                warn!("Failed to convert su_path: {}", e);
            }
        },
        Err(e) => {
            warn!("Failed to read su_path file: {}", e);
        }
    }
}
