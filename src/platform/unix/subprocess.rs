use crate::{entry, Duplex, Object, asynchronous::AsyncStream};
use rustix::process::Pid;
use std::ffi::{CStr, CString};
use std::io::Result;
use std::os::unix::io::{AsRawFd, BorrowedFd};

pub(crate) unsafe fn _spawn_child<S: Object, R: Object>(
    child_fd: Duplex<S, R>,
    inherited_fds: &[BorrowedFd<'_>],
) -> Result<Pid> {
    let child_fd_str = CString::new(child_fd.as_raw_fd().to_string()).unwrap();

    let spawn_cb = || {
        // Use abort() instead of panic!() to prevent stack unwinding, as unwinding in the fork
        // child may free resources that would later be freed in the original process
        match fork_child_main(child_fd.0.fd.as_handle(), &child_fd_str, inherited_fds) {
            Ok(()) => unreachable!(),
            Err(e) => {
                eprintln!("{e}");
                std::process::abort();
            }
        }
    };

    let mut stack = [0u8; 4096];
    Ok(Pid::from_raw(
        nix::sched::clone(
            Box::new(spawn_cb),
            &mut stack,
            nix::sched::CloneFlags::CLONE_VM | nix::sched::CloneFlags::CLONE_VFORK,
            Some(nix::libc::SIGCHLD),
        )?
        .as_raw(),
    )
    .unwrap())
}

unsafe fn fork_child_main(
    child_fd: BorrowedFd<'_>,
    child_fd_str: &CStr,
    inherited_fds: &[BorrowedFd<'_>],
) -> Result<()> {
    // No heap allocations are allowed here.
    entry::disable_cloexec(child_fd)?;
    for fd in inherited_fds {
        entry::disable_cloexec(*fd)?;
    }

    libc::execv(
        c"/proc/self/exe".as_ptr(),
        &[
            c"_crossmist_".as_ptr(),
            child_fd_str.as_ptr(),
            std::ptr::null(),
        ] as *const *const libc::c_char,
    );

    Err(std::io::Error::last_os_error())
}
