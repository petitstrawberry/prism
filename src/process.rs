use std::collections::HashSet;
use std::ffi::CStr;
use std::mem;

#[derive(Debug, Clone)]
pub struct ProcessIdentity {
    pub pid: i32,
    pub executable_path: Option<String>,
    pub display_name: Option<String>,
}

impl ProcessIdentity {
    fn from_pid(pid: i32) -> Option<Self> {
        let path = process_path(pid);
        let display_name = path
            .as_ref()
            .and_then(|p| p.rsplit('/').next().map(|segment| segment.to_string()))
            .filter(|name| !name.is_empty());

        Some(Self {
            pid,
            executable_path: path,
            display_name,
        })
    }

    pub fn preferred_name(&self) -> Option<String> {
        if let Some(name) = &self.display_name {
            return Some(name.clone());
        }

        self.executable_path
            .as_ref()
            .and_then(|path| path.rsplit('/').next().map(|segment| segment.to_string()))
    }
}

pub fn process_name(pid: i32) -> Option<String> {
    ProcessIdentity::from_pid(pid).and_then(|identity| identity.display_name)
}

pub fn process_path(pid: i32) -> Option<String> {
    if pid <= 0 {
        return None;
    }

    const BUF_SIZE: usize = 4096;
    let mut buffer = [0u8; BUF_SIZE];
    let ret = unsafe {
        libc::proc_pidpath(
            pid,
            buffer.as_mut_ptr() as *mut libc::c_void,
            BUF_SIZE as u32,
        )
    };

    if ret <= 0 {
        return None;
    }

    let cstr = unsafe { CStr::from_ptr(buffer.as_ptr() as *const libc::c_char) };
    Some(cstr.to_string_lossy().into_owned())
}

pub fn resolve_responsible_identity(pid: i32) -> Option<ProcessIdentity> {
    if pid <= 0 {
        return None;
    }

    let responsible_pid = find_responsible_pid(pid).unwrap_or(pid);
    ProcessIdentity::from_pid(responsible_pid)
}

pub fn find_responsible_pid(pid: i32) -> Option<i32> {
    if pid <= 0 {
        return None;
    }

    // Prefer the private responsibility API so helpers collapse under their owning app.
    if let Some(responsible) = unsafe { query_responsible_pid(pid) } {
        if responsible > 0 {
            return Some(responsible);
        }
    }

    follow_parent_chain(pid)
}

unsafe fn query_responsible_pid(pid: i32) -> Option<i32> {
    #[allow(non_snake_case)]
    extern "C" {
        fn responsibility_get_pid_responsible_for_pid(pid: libc::pid_t) -> libc::pid_t;
    }

    let result = responsibility_get_pid_responsible_for_pid(pid);
    if result > 0 {
        Some(result as i32)
    } else {
        None
    }
}

fn follow_parent_chain(start_pid: i32) -> Option<i32> {
    let mut current = start_pid;
    let mut last_good = start_pid;
    let mut visited = HashSet::new();

    // Walk up the BSD parent links as a fallback. Stops when we detect loops,
    // hit launchd, or encounter an .app executable path.
    while let Some(parent) = parent_pid(current) {
        if parent <= 0 || !visited.insert(parent) {
            break;
        }

        if parent == 1 {
            last_good = parent;
            break;
        }

        if let Some(path) = process_path(parent) {
            if is_probably_app_executable(&path) {
                return Some(parent);
            }
        }

        last_good = parent;
        current = parent;
    }

    Some(last_good)
}

fn parent_pid(pid: i32) -> Option<i32> {
    const PROC_PIDT_SHORTBSDINFO: libc::c_int = 13;

    #[repr(C)]
    #[derive(Clone, Copy)]
    struct ProcBsdShortInfo {
        pbsi_pid: u32,
        pbsi_ppid: u32,
        pbsi_pgid: u32,
        pbsi_status: u32,
        pbsi_comm: [libc::c_char; 16],
        pbsi_flags: u32,
        pbsi_uid: u32,
        pbsi_gid: u32,
        pbsi_ruid: u32,
        pbsi_rgid: u32,
        pbsi_svuid: u32,
        pbsi_svgid: u32,
        pbsi_rfu: u32,
    }

    let mut info = ProcBsdShortInfo {
        pbsi_pid: 0,
        pbsi_ppid: 0,
        pbsi_pgid: 0,
        pbsi_status: 0,
        pbsi_comm: [0; 16],
        pbsi_flags: 0,
        pbsi_uid: 0,
        pbsi_gid: 0,
        pbsi_ruid: 0,
        pbsi_rgid: 0,
        pbsi_svuid: 0,
        pbsi_svgid: 0,
        pbsi_rfu: 0,
    };

    let size = mem::size_of::<ProcBsdShortInfo>();
    let result = unsafe {
        libc::proc_pidinfo(
            pid,
            PROC_PIDT_SHORTBSDINFO,
            0,
            &mut info as *mut _ as *mut libc::c_void,
            size as i32,
        )
    };

    if result as usize == size {
        Some(info.pbsi_ppid as i32)
    } else {
        None
    }
}

fn is_probably_app_executable(path: &str) -> bool {
    path.contains(".app/Contents/MacOS/")
}
