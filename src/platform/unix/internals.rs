use crate::{Deserializer, Object, Serializer};
use rustix::{
    cmsg_space,
    net::{
        self, AddressFamily, RecvAncillaryBuffer, RecvAncillaryMessage, RecvFlags,
        SendAncillaryBuffer, SendAncillaryMessage, SendFlags, SocketFlags, SocketType, recvmsg,
        sendmsg,
    },
};
use std::io::{Error, ErrorKind, IoSlice, IoSliceMut, Result};
use std::marker::PhantomData;
use std::mem::MaybeUninit;
use std::os::unix::{
    io::{BorrowedFd, OwnedFd},
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
    fds: Vec<BorrowedFd<'a>>,
    buffer: Vec<u8>,
    data_pos: usize,
    fds_pos: usize,
    flags: SendFlags,
}

impl<'a> SingleObjectSender<'a> {
    pub(crate) fn new<T: Object>(socket_fd: BorrowedFd<'a>, value: &'a T, blocking: bool) -> Self {
        let mut s = Serializer::new();
        s.serialize(value);
        let fds = s.drain_handles();
        let buffer = s.into_vec();

        Self {
            socket_fd,
            fds,
            buffer,
            data_pos: 0,
            fds_pos: 0,
            flags: if blocking {
                SendFlags::empty()
            } else {
                SendFlags::DONTWAIT
            },
        }
    }

    pub(crate) fn send_next(&mut self) -> Result<()> {
        let mut space = [MaybeUninit::uninit(); cmsg_space!(ScmRights(MAX_PACKET_FDS))];
        let mut cmsg_buffer = SendAncillaryBuffer::new(&mut space);

        loop {
            let buffer_end = self.buffer.len().min(self.data_pos + MAX_PACKET_SIZE - 1);
            let fds_end = self.fds.len().min(self.fds_pos + MAX_PACKET_FDS);

            let is_last = buffer_end == self.buffer.len() && fds_end == self.fds.len();

            cmsg_buffer.clear();
            assert!(cmsg_buffer.push(SendAncillaryMessage::ScmRights(
                &self.fds[self.fds_pos..fds_end],
            )));

            let n_written = sendmsg(
                self.socket_fd,
                &[
                    IoSlice::new(&[is_last as u8]),
                    IoSlice::new(&self.buffer[self.data_pos..buffer_end]),
                ],
                &mut cmsg_buffer,
                self.flags,
            )?;

            self.data_pos += n_written - 1;
            self.fds_pos = fds_end;

            if is_last {
                return Ok(());
            }
        }
    }
}

pub(crate) struct SingleObjectReceiver<'a, T: Object> {
    socket_fd: BorrowedFd<'a>,
    buffer: Vec<u8>,
    data_pos: usize,
    fds: Vec<OwnedFd>,
    flags: RecvFlags,
    terminated: bool,
    marker: PhantomData<fn() -> T>,
}

unsafe impl<T: Object> Send for SingleObjectReceiver<'_, T> {}

impl<'a, T: Object> SingleObjectReceiver<'a, T> {
    pub(crate) unsafe fn new(socket_fd: BorrowedFd<'a>, blocking: bool) -> Self {
        Self {
            socket_fd,
            buffer: Vec::new(),
            data_pos: 0,
            fds: Vec::new(),
            flags: if blocking {
                RecvFlags::empty()
            } else {
                RecvFlags::DONTWAIT
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

        let mut space = [MaybeUninit::uninit(); cmsg_space!(ScmRights(MAX_PACKET_FDS))];
        let mut cmsg_buffer = RecvAncillaryBuffer::new(&mut space);

        loop {
            self.buffer.resize(self.data_pos + MAX_PACKET_SIZE - 1, 0);

            let mut marker = [0];
            let mut iovecs = [
                IoSliceMut::new(&mut marker),
                IoSliceMut::new(&mut self.buffer[self.data_pos..]),
            ];

            let message = recvmsg(
                self.socket_fd,
                &mut iovecs,
                &mut cmsg_buffer,
                self.flags | RecvFlags::CMSG_CLOEXEC,
            )?;

            for cmsg in cmsg_buffer.drain() {
                let RecvAncillaryMessage::ScmRights(rights) = cmsg else {
                    return Err(Error::other("Unexpected kind of cmsg on stream"));
                };
                self.fds.extend(rights);
            }

            if message.bytes == 0 {
                if self.data_pos == 0 && self.fds.is_empty() {
                    return Ok(None);
                } else {
                    return Err(Error::other("Unterminated data on stream"));
                }
            }

            if message.bytes == 0 {
                return Err(Error::other("Unexpected empty message on stream"));
            }

            self.data_pos += message.bytes - 1;
            if marker[0] != 1 {
                continue;
            }

            self.terminated = true;

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
