use crate::{imp::implements, pod::PlainOldData, Deserializer, Object, Serializer};
use nix::{
    cmsg_space,
    sys::socket::{recvmsg, sendmsg, ControlMessage, ControlMessageOwned, MsgFlags},
};
use rustix::net::{self, AddressFamily, SocketFlags, SocketType};
use std::io::{Error, ErrorKind, IoSlice, IoSliceMut, Result};
use std::marker::PhantomData;
use std::mem::MaybeUninit;
use std::os::unix::{
    io::{AsRawFd, BorrowedFd, FromRawFd, OwnedFd, RawFd},
    net::UnixStream,
};

pub(crate) const MAX_PACKET_SIZE: usize = 16 * 1024;
pub(crate) const MAX_PACKET_FDS: usize = 253; // SCM_MAX_FD

pub(crate) fn socketpair() -> Result<(UnixStream, UnixStream)> {
    // UnixStream creates a SOCK_STREAM by default, while we need SOCK_SEQPACKET
    let (tx, rx) = net::socketpair(
        AddressFamily::UNIX,
        SocketType::SEQPACKET,
        SocketFlags::CLOEXEC,
        None,
    )?;
    Ok((tx.into(), rx.into()))
}

pub(crate) struct SingleObjectSender<'a> {
    socket_fd: BorrowedFd<'a>,
    bytes: &'a [u8],
    fds: Vec<RawFd>,
    buffer: Vec<u8>,
    data_pos: usize,
    fds_pos: usize,
    flags: MsgFlags,
}

impl<'a> SingleObjectSender<'a> {
    pub(crate) fn new<T: Object>(socket_fd: BorrowedFd<'a>, value: &'a T, blocking: bool) -> Self {
        let bytes;
        let fds;
        let buffer;
        if implements!(T: PlainOldData) {
            bytes = unsafe {
                std::slice::from_raw_parts(value as *const T as *const u8, std::mem::size_of::<T>())
            };
            fds = Vec::new();
            buffer = Vec::new();
        } else {
            bytes = &[];
            let mut s = Serializer::new();
            s.serialize(value);
            fds = s.drain_handles();
            buffer = s.into_vec();
        }
        Self {
            socket_fd,
            bytes,
            fds,
            buffer,
            data_pos: 0,
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
            let buffer_end = self.data().len().min(self.data_pos + MAX_PACKET_SIZE - 1);
            let fds_end = self.fds.len().min(self.fds_pos + MAX_PACKET_FDS);

            let is_last = buffer_end == self.data().len() && fds_end == self.fds.len();

            let n_written = sendmsg::<()>(
                self.socket_fd.as_raw_fd(),
                &[
                    IoSlice::new(&[is_last as u8]),
                    IoSlice::new(&self.data()[self.data_pos..buffer_end]),
                ],
                &[ControlMessage::ScmRights(&self.fds[self.fds_pos..fds_end])],
                self.flags,
                None,
            )?;

            self.data_pos += n_written - 1;
            self.fds_pos = fds_end;

            if is_last {
                return Ok(());
            }
        }
    }

    fn data(&self) -> &[u8] {
        if self.bytes.is_empty() {
            &self.buffer
        } else {
            self.bytes
        }
    }
}

pub(crate) struct SingleObjectReceiver<T: Object> {
    socket_fd: RawFd,
    buffer: Vec<u8>,
    data_pos: usize,
    value: MaybeUninit<T>,
    fds: Vec<OwnedFd>,
    flags: MsgFlags,
    terminated: bool,
    marker: PhantomData<fn() -> T>,
}

unsafe impl<T: Object> Send for SingleObjectReceiver<T> {}

impl<T: Object> SingleObjectReceiver<T> {
    pub(crate) unsafe fn new(socket_fd: RawFd, blocking: bool) -> Self {
        Self {
            socket_fd,
            buffer: Vec::new(),
            data_pos: 0,
            value: MaybeUninit::zeroed(),
            fds: Vec::new(),
            flags: if blocking {
                MsgFlags::empty()
            } else {
                MsgFlags::MSG_DONTWAIT
            },
            terminated: false,
            marker: PhantomData,
        }
    }

    pub(crate) fn recv_next(&mut self) -> Result<Option<T>> {
        assert!(
            !self.terminated,
            "Calling recv_next after it returned Ok(Some(...)) or Err(...) is undefined behavior",
        );

        loop {
            if !implements!(T: PlainOldData) {
                self.buffer.resize(self.data_pos + MAX_PACKET_SIZE - 1, 0);
            }

            let data = if implements!(T: PlainOldData) {
                unsafe {
                    std::slice::from_raw_parts_mut(
                        self.value.as_mut_ptr() as *mut u8,
                        std::mem::size_of::<T>(),
                    )
                }
            } else {
                &mut self.buffer
            };

            let mut marker = [0];
            let mut iovecs = [
                IoSliceMut::new(&mut marker),
                IoSliceMut::new(&mut data[self.data_pos..]),
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
                if self.data_pos == 0 && self.fds.is_empty() {
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

            self.data_pos += message.bytes - 1;
            if marker[0] != 1 {
                continue;
            }

            self.terminated = true;

            if implements!(T: PlainOldData) {
                return Ok(Some(unsafe { self.value.assume_init_read() }));
            }

            self.buffer.truncate(self.data_pos);
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
