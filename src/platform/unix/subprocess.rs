use core::mem::MaybeUninit;
use libc::c_char;
use rustix::process::Pid;
use std::ffi::CString;
use std::io::{Error, Result};
use std::os::unix::io::{AsRawFd, BorrowedFd};

// `libc` doesn't export `environ` because POSIX says it's not part of any header:
// https://github.com/rust-lang/libc/pull/5339#discussion_r3677981017
unsafe extern "C" {
    static mut environ: *mut *mut c_char;
}

fn from_errno(errno: i32) -> Result<()> {
    if errno == 0 {
        Ok(())
    } else {
        Err(Error::from_raw_os_error(errno))
    }
}

struct FileActions(MaybeUninit<libc::posix_spawn_file_actions_t>);

impl FileActions {
    fn new() -> Result<Self> {
        let mut file_actions = MaybeUninit::uninit();
        from_errno(unsafe { libc::posix_spawn_file_actions_init(file_actions.as_mut_ptr()) })?;
        Ok(Self(file_actions))
    }

    fn as_mut_ptr(&mut self) -> *mut libc::posix_spawn_file_actions_t {
        self.0.as_mut_ptr()
    }

    fn as_ptr(&self) -> *const libc::posix_spawn_file_actions_t {
        self.0.as_ptr()
    }
}

impl Drop for FileActions {
    fn drop(&mut self) {
        from_errno(unsafe { libc::posix_spawn_file_actions_destroy(self.0.as_mut_ptr()) })
            .expect("posix_spawn_file_actions_destroy failed");
    }
}

pub(crate) unsafe fn _spawn_child(child_fd: BorrowedFd<'_>) -> Result<Pid> {
    let mut pid = 0;

    let child_fd_str = CString::new(child_fd.as_raw_fd().to_string()).unwrap();
    let argv = [
        c"_crossmist_".as_ptr(),
        child_fd_str.as_ptr(),
        core::ptr::null(),
    ];

    let mut file_actions = FileActions::new()?;

    from_errno(unsafe {
        libc::posix_spawn_file_actions_adddup2(
            file_actions.as_mut_ptr(),
            child_fd.as_raw_fd(),
            child_fd.as_raw_fd(),
        )
    })?;

    from_errno(unsafe {
        libc::posix_spawn(
            &raw mut pid,
            c"/proc/self/exe".as_ptr(),
            file_actions.as_ptr(),
            core::ptr::null(),
            &raw const argv as *const *mut c_char,
            environ,
        )
    })?;

    Ok(Pid::from_raw(pid).unwrap())
}
