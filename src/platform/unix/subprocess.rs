//! Management of child processes.
//!
//! To *start* a child process, you use the `spawn` method generated by `#[func]`:
//!
//! ```ignore
//! #[func]
//! fn my_process() {
//!     ...
//! }
//!
//! let child = my_process.spawn()?;
//! ```
//!
//! This module is about what happens *after* the child process is started: you can kill the child,
//! get its PID, or join it (i.e. wait till it returns and obtain the returned value).

use crate::{duplex, entry, imp, FnOnceObject, Object, Receiver, Serializer};
use nix::{
    libc::{c_char, c_int, c_void, pid_t},
    sys::signal,
};
use std::ffi::CString;
use std::io::Result;
use std::os::unix::io::{AsRawFd, RawFd};

#[doc(hidden)]
pub type Flags = c_int;

/// The subprocess object created by calling `spawn` on a function annottated with `#[func]`.
pub struct Child<T: Object> {
    proc_pid: nix::unistd::Pid,
    output_rx: Receiver<T>,
}

impl<T: Object> Child<T> {
    pub(crate) fn new(proc_pid: nix::unistd::Pid, output_rx: Receiver<T>) -> Child<T> {
        Child {
            proc_pid,
            output_rx,
        }
    }

    /// Terminate the process immediately.
    pub fn kill(&mut self) -> Result<()> {
        signal::kill(self.proc_pid, signal::Signal::SIGKILL)?;
        Ok(())
    }

    /// Get ID of the process.
    pub fn id(&self) -> pid_t {
        self.proc_pid.as_raw()
    }

    /// Wait for the process to finish and obtain the value it returns.
    ///
    /// An error is returned if the process panics, is terminated, or exits via
    /// [`std::process::exit`] or alike instead of returning a
    /// value.
    pub fn join(&mut self) -> Result<T> {
        let value = self.output_rx.recv()?;
        let status = nix::sys::wait::waitpid(self.proc_pid, None)?;
        if let nix::sys::wait::WaitStatus::Exited(_, 0) = status {
            value.ok_or_else(|| {
                std::io::Error::new(
                    std::io::ErrorKind::Other,
                    "The subprocess terminated without returning a value",
                )
            })
        } else {
            Err(std::io::Error::new(
                std::io::ErrorKind::Other,
                format!(
                    "The subprocess did not terminate successfully: {:?}",
                    status
                ),
            ))
        }
    }
}

pub(crate) unsafe fn _spawn_child(
    child_fd: RawFd,
    flags: Flags,
    inherited_fds: &[RawFd],
) -> Result<nix::unistd::Pid> {
    let child_fd_str = CString::new(child_fd.to_string()).unwrap();

    match nix::libc::syscall(
        nix::libc::SYS_clone,
        nix::libc::SIGCHLD | flags,
        std::ptr::null::<c_void>(),
    ) {
        -1 => Err(std::io::Error::last_os_error()),
        0 => {
            // No heap allocations are allowed from now on
            let res: Result<!> = try {
                signal::sigprocmask(
                    signal::SigmaskHow::SIG_SETMASK,
                    Some(&signal::SigSet::empty()),
                    None,
                )?;
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

                Err(std::io::Error::last_os_error())?;

                unreachable!()
            };

            // Use abort() instead of panic!() to prevent stack unwinding, as unwinding in the fork
            // child may free resources that would later be freed in the original process
            eprintln!("{}", res.into_err());
            std::process::abort();
        }
        child_pid => Ok(nix::unistd::Pid::from_raw(child_pid as pid_t)),
    }
}

#[doc(hidden)]
pub unsafe fn spawn<T: Object>(
    entry: Box<dyn FnOnceObject<(RawFd,), Output = i32>>,
    flags: Flags,
) -> Result<Child<T>> {
    imp::perform_sanity_checks();

    let mut s = Serializer::new();
    s.serialize(&entry);

    let fds = s.drain_handles();

    let (mut local, child) = duplex::<(Vec<u8>, Vec<RawFd>), T>()?;
    let pid = _spawn_child(child.as_raw_fd(), flags, &fds)?;
    local.send(&(s.into_vec(), fds))?;

    Ok(Child::new(pid, local.into_receiver()))
}
