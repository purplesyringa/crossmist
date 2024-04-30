use crate::{Deserializer, Object, Serializer};
use nix::libc::{AF_UNIX, SOCK_CLOEXEC, SOCK_SEQPACKET};
use nix::{
    cmsg_space,
    sys::socket::{recvmsg, sendmsg, ControlMessage, ControlMessageOwned, MsgFlags},
};
use std::io::{Error, ErrorKind, IoSlice, IoSliceMut, Result};
use std::marker::PhantomData;
use std::os::unix::{
    io::{FromRawFd, OwnedFd, RawFd},
    net::UnixStream,
};

pub(crate) const MAX_PACKET_SIZE: usize = 16 * 1024;
pub(crate) const MAX_PACKET_FDS: usize = 253; // SCM_MAX_FD

pub(crate) fn socketpair(flags: i32) -> Result<(UnixStream, UnixStream)> {
    // UnixStream creates a SOCK_STREAM by default, while we need SOCK_SEQPACKET
    unsafe {
        let mut fds = [0, 0];
        if nix::libc::socketpair(
            AF_UNIX,
            SOCK_SEQPACKET | SOCK_CLOEXEC | flags,
            0,
            fds.as_mut_ptr(),
        ) == -1
        {
            return Err(std::io::Error::last_os_error());
        }
        Ok((
            UnixStream::from_raw_fd(fds[0]),
            UnixStream::from_raw_fd(fds[1]),
        ))
    }
}

pub(crate) struct SingleObjectSender {
    socket_fd: RawFd,
    fds: Vec<RawFd>,
    buffer: Vec<u8>,
    buffer_pos: usize,
    fds_pos: usize,
    flags: MsgFlags,
}

impl SingleObjectSender {
    pub(crate) fn new<T: Object>(socket_fd: RawFd, value: &T, blocking: bool) -> Self {
        let mut s = Serializer::new();
        s.serialize(value);
        Self {
            socket_fd,
            fds: s.drain_handles(),
            buffer: s.into_vec(),
            buffer_pos: 0,
            fds_pos: 0,
            flags: if blocking {
                MsgFlags::empty()
            } else {
                MsgFlags::MSG_DONTWAIT
            },
        }
    }

    pub(crate) fn send_next(&mut self) -> Result<()> {
        loop {
            let buffer_end = self.buffer.len().min(self.buffer_pos + MAX_PACKET_SIZE - 1);
            let fds_end = self.fds.len().min(self.fds_pos + MAX_PACKET_FDS);

            let is_last = buffer_end == self.buffer.len() && fds_end == self.fds.len();

            let n_written = sendmsg::<()>(
                self.socket_fd,
                &[
                    IoSlice::new(&[is_last as u8]),
                    IoSlice::new(&self.buffer[self.buffer_pos..buffer_end]),
                ],
                &[ControlMessage::ScmRights(&self.fds[self.fds_pos..fds_end])],
                self.flags,
                None,
            )?;

            self.buffer_pos += n_written - 1;
            self.fds_pos = fds_end;

            if is_last {
                return Ok(());
            }
        }
    }
}

pub(crate) struct SingleObjectReceiver<T: Object> {
    socket_fd: RawFd,
    buffer: Vec<u8>,
    buffer_pos: usize,
    fds: Vec<OwnedFd>,
    flags: MsgFlags,
    marker: PhantomData<fn() -> T>,
}

impl<T: Object> SingleObjectReceiver<T> {
    pub(crate) unsafe fn new(socket_fd: RawFd, blocking: bool) -> Self {
        Self {
            socket_fd,
            buffer: Vec::new(),
            buffer_pos: 0,
            fds: Vec::new(),
            flags: if blocking {
                MsgFlags::empty()
            } else {
                MsgFlags::MSG_DONTWAIT
            },
            marker: PhantomData,
        }
    }

    pub(crate) fn recv_next(&mut self) -> Result<Option<T>> {
        loop {
            self.buffer.resize(self.buffer_pos + MAX_PACKET_SIZE - 1, 0);

            let mut marker = [0];
            let mut iovecs = [
                IoSliceMut::new(&mut marker),
                IoSliceMut::new(&mut self.buffer[self.buffer_pos..]),
            ];

            let mut ancillary = cmsg_space!([RawFd; MAX_PACKET_FDS]);

            let message = recvmsg::<()>(
                self.socket_fd,
                &mut iovecs,
                Some(&mut ancillary),
                self.flags | MsgFlags::MSG_CMSG_CLOEXEC,
            )?;

            for cmsg in message.cmsgs() {
                if let ControlMessageOwned::ScmRights(rights) = cmsg {
                    for fd in rights {
                        self.fds.push(unsafe { OwnedFd::from_raw_fd(fd) });
                    }
                } else {
                    return Err(Error::new(
                        ErrorKind::Other,
                        "Unexpected kind of cmsg on stream",
                    ));
                }
            }

            if message.cmsgs().next().is_none() && message.bytes == 0 {
                if self.buffer_pos == 0 && self.fds.is_empty() {
                    return Ok(None);
                } else {
                    return Err(Error::new(ErrorKind::Other, "Unterminated data on stream"));
                }
            }

            if message.bytes == 0 {
                return Err(Error::new(
                    ErrorKind::Other,
                    "Unexpected empty message on stream",
                ));
            }

            self.buffer_pos += message.bytes - 1;
            if marker[0] == 1 {
                self.buffer.truncate(self.buffer_pos);
                let buffer = std::mem::take(&mut self.buffer);
                let fds = std::mem::take(&mut self.fds);
                let mut d = Deserializer::new(buffer, fds);
                return match unsafe { d.deserialize() } {
                    Ok(value) => Ok(Some(value)),
                    Err(e) if e.kind() == ErrorKind::WouldBlock => {
                        // Prevent this error from being interpreted as a "wait for socket" signal
                        Err(std::io::Error::other("Unexpected blocking event"))
                    }
                    Err(e) => Err(e),
                };
            }
        }
    }
}
