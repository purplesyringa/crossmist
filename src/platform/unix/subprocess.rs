use crate::{entry, Duplex, Object};
use nix::{libc::c_char, sched};
use rustix::process::Pid;
use std::ffi::{CStr, CString};
use std::io::Result;
use std::os::unix::io::{AsRawFd, RawFd};

pub(crate) unsafe fn _spawn_child<S: Object, R: Object>(
    child_fd: Duplex<S, R>,
    inherited_fds: &[RawFd],
) -> Result<Pid> {
    let child_fd_str = CString::new(child_fd.as_raw_fd().to_string()).unwrap();

    let spawn_cb = || {
        // Use abort() instead of panic!() to prevent stack unwinding, as unwinding in the fork
        // child may free resources that would later be freed in the original process
        match fork_child_main(child_fd.as_raw_fd(), &child_fd_str, inherited_fds) {
            Ok(()) => unreachable!(),
            Err(e) => {
                eprintln!("{e}");
                std::process::abort();
            }
        }
    };

    let mut stack = [0u8; 4096];
    Ok(Pid::from_raw(
        sched::clone(
            Box::new(spawn_cb),
            &mut stack,
            sched::CloneFlags::CLONE_VM | sched::CloneFlags::CLONE_VFORK,
            Some(nix::libc::SIGCHLD),
        )?
        .as_raw(),
    )
    .unwrap())
}

unsafe fn fork_child_main(
    child_fd: RawFd,
    child_fd_str: &CStr,
    inherited_fds: &[RawFd],
) -> Result<()> {
    // No heap allocations are allowed here.
    entry::disable_cloexec(child_fd)?;
    for fd in inherited_fds {
        entry::disable_cloexec(*fd)?;
    }

    // nix::unistd::execv uses allocations
    nix::libc::execv(
        c"/proc/self/exe".as_ptr(),
        &[
            c"_crossmist_".as_ptr(),
            child_fd_str.as_ptr(),
            std::ptr::null(),
        ] as *const *const c_char,
    );

    Err(std::io::Error::last_os_error())
}
