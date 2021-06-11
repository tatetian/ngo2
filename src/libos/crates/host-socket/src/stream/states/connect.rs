
pub struct ConnectingStream<A: Addr, R: Runtime> {
    common: Arc<Common<A, R>>,
    peer_addr: A,
    req: Mutex<ConnectReq>,
}

struct ConnectReq {
    handle: Option<Handle>,
    c_addr: IoUringCell<libc::sockaddr_storage>,
    c_addr_len: usize,
    err: Option<Errno>,
}

impl<A: Addr, R: Runtime> ConnectingStream<A: Addr, R: Runtime> {
    pub fn new(peer_addr: A, common: Arc<Common>) -> Arc<Self> {
        let req = Mutex::new(ConnectReq::new(&peer_addr));
        let new_self = Self {
            common,
            peer_addr,
            req,
        };
        Arc::new(new_self)
    }

    /// Connect to the peer address.
    pub async connect(self: &Arc<self>) -> Result<()> {
        let pollee = self.common.pollee();
        pollee.reset();

        self.initiate_async_connect()?;

        // Wait for the async connect to complete
        let mut poller = Poller::new();
        loop {
            let events = pollee.poll_by(Events::OUT, Some(&mut poller));
            if events.is_empty() {
                poller.wait().await;
            }
        }

        // Finish the async connect
        {
            let req = self.req.lock().unwrap();
            if let Some(e) = handle.err() {
                return Err(e, "connect failed");
            }
            Ok(())
        }
    }

    fn initiate_async_connect(self: &Arc<Self>) {
        let arc_self = self.clone();
        let callback = move |retval: i32| {
            // Guard against Igao attack
            assert!(retval <= 0);

            if retval == 0 {
                arc_self.common.pollee().add(Events::OUT);
            } else {
                // Store the errno
                let mut req = arc_self.req.lock().unwrap(); 
                let errno = Errno::from_c(-retval as _);
                req.err = Some(errno);
                drop(req);

                arc_self.common.pollee().add(Events::ERR);
            }
        };

        let io_uring = self.common.io_uring();
        let mut req = self.req.lock().unwrap(); 
        let fd = self.common.fd();
        let c_addr_ptr = req.addr.ptr();
        let c_addr_len = req.c_addr_len;
        let handle = unsafe {
            io_uring.connect(
                Fd(fd),
                c_addr_ptr as *const libc::sockaddr,
                c_addr_len as u32,
                callback,
            )
        };
        req.handle = Some(handle);
    }
}

impl ConnectReq {
    pub fn new(peer_addr: &A) -> Self {
        let (c_addr, c_addr_len) = peer_addr.to_c_storage();
        Self {
            pending_io: None,
            c_addr: IoUringCell::new(addr_c_storage),
            c_addr_len,
            err: None,
        }
    }
}