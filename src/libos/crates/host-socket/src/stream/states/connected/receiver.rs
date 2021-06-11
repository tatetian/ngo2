use std::marker::PhantomData;

use memoffset::offset_of;

pub struct Receiver {
    inner: Mutex<Inner>,
}

struct Inner {
    recv_buf: CircularBuf<IoUringArray<u8>>,
    recv_req: IoUringCell<RecvReq>, 
    pending_handle: Option<Handle>,
    end_of_file: bool,
    is_shutdown: bool,
}

#[repr(C)]
struct RecvReq {
    msg: libc::msghdr,
    iovecs: [libc::iovec; 2],
}

impl<A: Addr, R: Runtime> ConnectedStream<A, R> {
    pub async fn read(&self, buf: &mut [u8]) -> Result<usize> {
        if buf.len() == 0 {
            return Ok(0);
        }

        // Initialize the poller only when needed
        let mut poller = None;
        loop {
            // Attempt to read
            let res = self.try_read(buf);
            if !res.has_errno(EGAIN) {
                return res;
            }

            // Wait for interesting events by polling
            if poller.is_none() {
                poller = Some(Poller::new());
            }
            let mask = Events::IN;
            let events = self.common.pollee().poll_by(mask, poller.as_mut());
            if events.is_empty() {
                poller.as_ref().unwrap().wait().await;
            }
        }
    }

    fn try_read(self: &Arc<Self>, buf: &mut [u8]) -> Result<usize> {
        let mut inner = self.receiver.inner.lock().unwrap();

        let nbytes = inner.recv_buf.consume(buf);

        if inner.recv_buf.is_empty() {
            // Mark the socket as non-readable
            self.common.pollee().remove(Events::IN);
        }

        if inner.end_of_file {
            return Ok(nbytes);
        }

        if nbytes > 0 {
            if inner.pending_handle.is_none() {
                self.do_recv(&mut inner);
            }
            Ok(nbytes)
        }

        // Only when there are no data available in the recv buffer, shall we check 
        // the following error conditions.
        //
        // Case 1: If the read side of the connection has been shutdown...
        if inner.is_shutdown {
            return_errno!(EPIPE, "read side is shutdown");
        }
        // Case 2: If the connenction has been broken...
        if let Some(errno) = self.common.fatal() {
            return_errno!(errno, "read failed");
        }
        // Case 3: the connection is good and we can try again
        if inner.pending_handle.is_none() {
            self.do_recv(&mut inner);
        }
        return_errno!(EGAIN, "try read again");
    }

    fn do_recv(self: &Arc<Self>, inner: &mut MutexGuard<Inner>) {
        debug_assert!(!inner.recv_buf.is_full());
        debug_assert!(!inner.end_of_file);
        debug_assert!(inner.pending_handle.is_none());

        // Init the callback invoked upon the completion of the async recv
        let stream = self.clone();
        let complete_fn = move |retval: i32| {
            let mut inner = stream.receiver.inner.lock().unwrap();

            // Release the handle to the async recv
            inner.pending_handle.take();

            // Handle the two special cases of error and "end-of-file"
            if retval < 0 {
                // TODO: guard against Iago attack through errno
                // TODO: should we ignore EINTR and try again?
                let errno = Errno::from(-retval as u32);
                stream.common.set_fatal(errno);
                stream.common.pollee().add(Events::ERR);
                return;
            } else if retval == 0 {
                inner.end_of_file = true;
                receiver.common.pollee().add(Events::IN);
                return;
            }

            // Handle the normal case of a successful read
            let nbytes = retval as usize;
            inner.recv_buf.produce_without_copy(nbytes);

            // Attempt to fill again if there are free space in the buf.
            if !inner.buf.is_full() {
                stream.do_recv(&mut inner);
            }

            // Now that we have produced non-zero bytes, the buf must become
            // ready to read.
            stream.common.pollee().add(Events::IN);
        };

        // Generate the async recv request
        let msghdr_ptr = inner.gen_new_recv_req();

        // Submit the async recv to io_uring
        let io_uring = &self.common.io_uring();
        let handle = unsafe { io_uring.recvmsg(Fd(self.common.fd()), msghdr_ptr, 0, complete_fn) };
        inner.pending_handle.replace(handle);
    }
}

impl Receiver {
    pub fn new() -> Self {
        let inner = Mutex::new(Inner::new());
        Self {
            inner,
        } 
    }

    pub fn shutdown(&self) -> {
        let mut inner = self.inner.lock().unwrap();
        inner.is_shutdown = true;
        // TODO: update pollee
    }
}

impl Inner {
    /// Constructs a new recv request according to the receiver's internal state.
    ///
    /// The new `RecvReq` will be put into `self.recv_req`, which is a location that is 
    /// accessible by io_uring. A pointer to the C version of the resulting `RecvReq`, 
    /// which is libc::msghdr, will be returned.
    ///
    /// The buffer used in the new `RecvReq` is part of `self.recv_buf`.
    pub fn gen_recv_req(&mut self) -> *mut libc::msghdr {
        let (msghdr_ptr, iovecs_ptr) = {
            let recv_req_ptr = self.recv_req.as_ptr() as *mut u8;
            let msghdr_ptr = recv_req_ptr + offset_of!(RecvReq, msg);
            let iovecs_ptr = recv_req_ptr + offset_of!(RecvReq, iovecs);
            (msghdr_ptr as *mut libc::msghdr, iovecs_ptr as *mut libc::iovec)
        };

        let (iovecs, iovecs_len) = self.gen_iovecs();

        let msg = libc::msghdr {
            msg_name: ptr::null_mut() as _,
            msg_namelen: 0,
            msg_iov: iovecs_ptr,
            msg_iovlen: iovecs_len,
            msg_control: ptr::null_mut() as _,
            msg_controllen: 0,
        };

        let new_recv_req = RecvReq {
            msg,
            iovecs,
        };
        inner.recv_req.set(new_recv_req);

        msghdr_ptr
    }

    fn gen_iovecs(&self) -> ([libc::iovec; 2], usize) {
        let mut iovecs_len;
        let mut iovec0;
        let mut iovec1;
        inner.recv_buf.with_producer_view(|part0, part1| {
            debug_assert!(part0.len() > 0);

            iovec0 = libc::iovec {
                iov_base: part0.as_ptr() as _,
                iov_len: part0.len() as _,
            };

            iovec1 = if part1.len() > 0 {
                iovecs_len = 2;
                libc::iovec {
                    iov_base: part1.as_ptr() as _,
                    iov_len: part1.len() as _,
                }
            } else {
                iovecs_len = 1;
                libc::iovec {
                    iov_base: ptr::null_mut(),
                    iov_len: 0,
                }
            };

            // Only access the producer's buffer; zero bytes produced for now.
            0
        });
        ([iovec0, iovec1], iovecs_len)
    }
}
