use crate::{asynchronous::AsyncStream, entry, Duplex, Object};
use libc::{c_char, c_int, c_void};
use rustix::process::Pid;
use std::ffi::{CStr, CString};
use std::io::Result;
use std::os::unix::io::{AsRawFd, BorrowedFd};

struct CloneArg<'a> {
    child_fd: BorrowedFd<'a>,
    child_fd_str: &'a CStr,
    inherited_fds: &'a [BorrowedFd<'a>],
}

pub(crate) unsafe fn _spawn_child<S: Object, R: Object>(
    child_fd: Duplex<S, R>,
    inherited_fds: &[BorrowedFd<'_>],
) -> Result<Pid> {
    let child_fd_str = CString::new(child_fd.as_raw_fd().to_string()).unwrap();
    let clone_arg = CloneArg {
        child_fd: child_fd.0.fd.as_handle(),
        child_fd_str: &child_fd_str,
        inherited_fds,
    };

    let mut stack = [0u8; 4096];
    let result = libc::clone(
        clone_callback,
        stack.as_mut_ptr_range().end as *mut c_void,
        libc::CLONE_VM | libc::CLONE_VFORK | libc::SIGCHLD,
        &clone_arg as *const CloneArg as *mut c_void,
    );

    if result < 0 {
        Err(std::io::Error::last_os_error())
    } else {
        Ok(Pid::from_raw(result as i32).unwrap())
    }
}

// XXX: The signature of libc::clone forces this function to be safe when in reality it isn't
// (calling it with an arbitrary arg may be unsound). libc 1.0 is going to fix that, see
// https://github.com/rust-lang/libc/issues/2198.
extern "C" fn clone_callback(arg: *mut c_void) -> c_int {
    // Use abort() instead of panic!() to prevent stack unwinding, as unwinding in the fork child
    // may free resources that would later be freed in the original process
    match fork_child_main(unsafe { &*(arg as *mut CloneArg) }) {
        Ok(()) => unreachable!(),
        Err(e) => {
            eprintln!("{e}");
            std::process::abort();
        }
    }
}

fn fork_child_main(arg: &CloneArg) -> Result<()> {
    // No heap allocations are allowed here.
    entry::disable_cloexec(arg.child_fd)?;
    for fd in arg.inherited_fds {
        entry::disable_cloexec(*fd)?;
    }

    unsafe {
        libc::execv(
            c"/proc/self/exe".as_ptr(),
            &[
                c"_crossmist_".as_ptr(),
                arg.child_fd_str.as_ptr(),
                std::ptr::null(),
            ] as *const *const c_char,
        );
    }

    Err(std::io::Error::last_os_error())
}
