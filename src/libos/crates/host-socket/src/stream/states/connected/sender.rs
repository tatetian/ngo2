use std::marker::PhantomData;

use memoffset::offset_of;

pub struct Sender {
    inner: Mutex<Inner>,
}

struct Inner {
    send_buf: CircularBuf<IoUringArray<u8>>,
    send_req: IoUringCell<SendReq>, 
    pending_handle: Option<Handle>,
    end_of_file: bool,
    is_shutdown: bool,
}

#[repr(C)]
struct SendReq {
    msg: libc::msghdr,
    iovecs: [libc::iovec; 2],
}

impl<A: Addr, R: Runtime> ConnectedStream<A, R> {
    pub async fn write(&self, buf: &[u8]) -> Result<usize> {
        if buf.len() == 0 {
            return Ok(0);
        }

        // Initialize the poller only when needed
        let mut poller = None;
        loop {
            // Attempt to write
            let res = self.try_write(buf);
            if !res.has_errno(EGAIN) {
                return res;
            }

            // Wait for interesting events by polling
            if poller.is_none() {
                poller = Some(Poller::new());
            }
            let mask = Events::OUT;
            let events = self.common.pollee().poll_by(mask, poller.as_mut());
            if events.is_empty() {
                poller.as_ref().unwrap().wait().await;
            }
        }
    }

    fn try_write(self: &Arc<Self>, buf: &[u8]) -> Result<usize> {
        let mut inner = self.sender.inner.lock().unwrap();

        // Check for error condition before write.
        //
        // Case 1. If the write side of the connection has been shutdown...
        if inner.is_shutdown {
            return_errno!(EPIPE, "write side is shutdown");
        }
        // Case 2. If the connenction has been broken...
        if let Some(errno) = self.common.fatal() {
            return_errno!(errno, "write failed");
        }

        let nbytes = inner.send_buf.produce(buf);

        if inner.send_buf.is_full() {
            // Mark the socket as non-writable
            self.common.pollee().remove(Events::OUT);
        }

        if nbytes > 0 {
            if inner.pending_handle.is_none() {
                self.do_send(&mut inner);
            }
            Ok(nbytes)
        }

        // Try again
        if inner.pending_handle.is_none() {
            self.do_send(&mut inner);
        }
        return_errno!(EGAIN, "try write again");
    }

    fn do_send(self: &Arc<Self>, inner: &mut MutexGuard<Inner>) {
        debug_assert!(!inner.send_buf.is_empty());
        debug_assert!(!inner.shutdown);
        debug_assert!(inner.pending_handle.is_none());

        // Init the callback invoked upon the completion of the async send
        let stream = self.clone();
        let complete_fn = move |retval: i32| {
            let mut inner = stream.sender.inner.lock().unwrap();

            // Release the handle to the async send
            inner.pending_handle.take();

            // Handle error
            if retval < 0 {
                // TODO: guard against Iago attack through errno
                // TODO: should we ignore EINTR and try again?
                let errno = Errno::from(-retval as u32);
                stream.common.set_fatal(errno);
                stream.common.pollee().add(Events::ERR);
                return;
            }
            assert!(retval != 0);

            // Handle the normal case of a successful write
            let nbytes = retval as usize;
            inner.send_buf.consume_without_copy(nbytes);

            // Attempt to send again if there are available data in the buf.
            if !inner.send_buf.is_empty() {
                stream.do_send(&mut inner);
            }

            // Now that we have consume non-zero bytes, the buf must become
            // ready to write.
            stream.common.pollee().add(Events::OUT);
        };

        // Generate the async send request
        let msghdr_ptr = inner.gen_new_send_req();

        // Submit the async send to io_uring
        let io_uring = &self.common.io_uring();
        let handle = unsafe { io_uring.sendmsg(Fd(self.common.fd()), msghdr_ptr, 0, complete_fn) };
        inner.pending_handle.replace(handle);
    }
}

impl Sender {
    pub fn new() -> Self {
        let inner = Mutex::new(Inner::new());
        Self {
            inner,
        } 
    }

    pub fn shutdown(&self) -> {
        let mut inner = self.inner.lock().unwrap();
        inner.is_shutdown = true;
    }
}

impl Inner {
    /// Constructs a new send request according to the sender's internal state.
    ///
    /// The new `SendReq` will be put into `self.send_req`, which is a location that is 
    /// accessible by io_uring. A pointer to the C version of the resulting `SendReq`, 
    /// which is libc::msghdr, will be returned.
    ///
    /// The buffer used in the new `SendReq` is part of `self.send_buf`.
    pub fn gen_send_req(&mut self) -> *mut libc::msghdr {
        let (msghdr_ptr, iovecs_ptr) = {
            let send_req_ptr = self.send_req.as_ptr() as *mut u8;
            let msghdr_ptr = send_req_ptr + offset_of!(SendReq, msg);
            let iovecs_ptr = send_req_ptr + offset_of!(SendReq, iovecs);
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

        let new_send_req = SendReq {
            msg,
            iovecs,
        };
        inner.send_req.set(new_send_req);

        msghdr_ptr
    }

    fn gen_iovecs(&self) -> ([libc::iovec; 2], usize) {
        let mut iovecs_len;
        let mut iovec0;
        let mut iovec1;
        inner.send_buf.with_consumer_view(|part0, part1| {
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

            // Only access the consumer's buffer; zero bytes consumed for now.
            0
        });
        ([iovec0, iovec1], iovecs_len)
    }
}
