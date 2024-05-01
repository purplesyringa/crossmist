use crate::{entry, Duplex, Object};
use nix::{libc::c_char, sched, sys::signal};
use std::ffi::{CStr, CString};
use std::io::Result;
use std::os::unix::io::{AsRawFd, RawFd};

pub(crate) unsafe fn _spawn_child<S: Object, R: Object>(
    child_fd: Duplex<S, R>,
    inherited_fds: &[RawFd],
) -> Result<nix::unistd::Pid> {
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
    Ok(sched::clone(
        Box::new(spawn_cb),
        &mut stack,
        sched::CloneFlags::CLONE_VM | sched::CloneFlags::CLONE_VFORK,
        Some(nix::libc::SIGCHLD),
    )?)
}

unsafe fn fork_child_main(
    child_fd: RawFd,
    child_fd_str: &CStr,
    inherited_fds: &[RawFd],
) -> Result<()> {
    // No heap allocations are allowed here.
    for i in 1..32 {
        if i != nix::libc::SIGKILL && i != nix::libc::SIGSTOP {
            signal::sigaction(
                signal::Signal::try_from(i).unwrap(),
                &signal::SigAction::new(
                    signal::SigHandler::SigDfl,
                    signal::SaFlags::empty(),
                    signal::SigSet::empty(),
                ),
            )?;
        }
    }
    signal::sigprocmask(
        signal::SigmaskHow::SIG_SETMASK,
        Some(&signal::SigSet::empty()),
        None,
    )?;

    entry::disable_cloexec(child_fd)?;
    for fd in inherited_fds {
        entry::disable_cloexec(*fd)?;
    }

    // nix::unistd::execv uses allocations
    nix::libc::execv(
        b"/proc/self/exe\0" as *const u8 as *const c_char,
        &[
            b"_crossmist_\0" as *const u8 as *const c_char,
            child_fd_str.as_ptr() as *const u8 as *const c_char,
            std::ptr::null(),
        ] as *const *const c_char,
    );

    Err(std::io::Error::last_os_error())
}
