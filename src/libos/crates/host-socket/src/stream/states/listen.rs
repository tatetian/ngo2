
/// A listener stream, ready to accept incoming connections.
pub struct ListenerStream<A: Addr, R: Runtime>  {
    common: Arc<Common<A, R>>, 
    addr: A,
    inner: Mutex<Inner<A>>,
}

/// The mutable, internal state of a listener stream.
struct Inner {
    backlog: Backlog<A>,
}

impl<A: Addr, R: Runtime> ListenerStream<A, R> {
    pub async fn accept(self: &Arc<Self>) -> Result<Stream<A, R>> {
        // Init the poller only when needed
        let mut poller = None;
        loop {
            // Attempt to accept
            let res = self.try_accept();
            if !res.has_errno(EAGAIN) {
                return res;
            }

            // Ensure the poller is initialized
            if poller.is_none() {
                poller = Some(Poller::new());
            }
            // Wait for interesting events by polling
            let mask = Events::IN;
            let events = self.common.pollee().poll_by(mask, poller.as_mut());
            if events.is_empty() {
                poller.as_ref().unwrap().wait().await;
            }
        }
    }

    pub fn try_accept(&self) -> Result<Stream<A, R>> {
        let mut inner = self.inner.lock().unwrap();

        let (accepted_fd, accepted_addr) = inner.backlog
            .pop_completed()
            .ok_or_else(|| errno!(EAGAIN, "try accept again"))?;
        
        if !inner.backlog.has_completed() {
            self.common.pollee().remove(Events::IN);
        }

        let accepted_stream = ConnectedStream::with_fd_and_peer(accepted_fd, accepted_addr).unwrap();
        self.initiate_async_accepts(inner);
        Ok(accepted_stream)
    }

    fn initiate_async_accepts(self: &Arc<Self>, inner: &mut MutexGuard<Inner>) {
        // We hold the following invariant:
        //
        //      backlog.capacity() >= backlog.num_pending() + backlog.num_completed()
        //
        // And for the maximal performance, we try to make the two sides equal.
        while inner.has_free_entries() {
            inner.start_new_req(self.clone());
        }
    }
}

/// A backlog of incoming connections of a listener stream.
///
/// With backlog, we can start async accept requests, keep track of the pending requests,
/// and maintain the ones that have completed.
struct Backlog<A> {
    entries: Box<[Entry]>,
    reqs: IoUringArray<AcceptReq>,
    completed: VecDeque<usize>,
    num_free: usize,
}

/// An entry in the backlog.
enum Entry {
    /// The entry is free to use.
    Free,
    /// The entry represents a pending accept request.
    Pending {
        handle: Handle,
    },
    /// The entry represents a completed accept request.
    Completed {
        fd: u32,
    },
}

#[repr(C)]
struct AcceptReq {
    c_addr: libc::sockaddr_storage,
    c_addr_len: libc::socklen_t,
}


impl<A: Addr> Backlog<A> {
    pub fn with_capacity(capacity: usize) -> Self {

    }
    
    pub fn has_free_entries(&self) -> usize {

    }

    pub fn has_completed_reqs(&self) -> usize {

    }

    /// Start a new async accept request, turning a free entry into a pending one.
    pub fn start_new_req(&mut self, stream: Arc<ListenerStream>) {
        debug_assert!(self.has_free());

        let entry_idx = self.entries.find(|entry| entry == Entry::Free).unwrap();
        let accept_req_ptr = self.reqs.as_ptr().add(entry_index);
        let c_addr_ptr = (accept_req_ptr as *mut u8).add(offset_of!(AcceptReq, c_addr)) as _;
        let c_addr_len_ptr = (accept_req_ptr as *mut u8).add(offset_of!(AcceptReq, c_addr_len)) as _;
        unsafe { *c_addr_len_ptr = std::mem::size_of::<libc::sockaddr_storage>(); }

        let callback = move |retval: i32| {
            let mut inner = stream.inner.lock().unwrap();

            if retval < 0 {
                let errno = Errno::from(-retval);
                stream.common.set_fatal(retval);
                stream.common.pollee().add(Events::ERR);

                // Free the resources allocated from the slabs
                let param = pending_accept.param();
                drop(pending_accept);
                unsafe { inner.param_raw_slab.dealloc(param) };
                inner.accept_slab.remove(accept_slab_index);

                backlog.entries[entry_idx] = Entry::Pending { handle };
                return;
            }

            let fd = retval;
            backlog.entries[entry_idx] = Entry::Completed { fd };
            stream.common.pollee().add(Events::IN);
        };

        let io_uring = self.common.io_uring();
        let fd = self.common.fd();
        let flags = 0;
        let handle = unsafe {
            io_uring.accept(
                Fd(fd),
                c_addr_ptr,
                c_addr_len_ptr,
                flags,
                callback,
            )
        };

        backlog.entries[entry_idx] = Entry::Pending { handle };
    }

    /// Pop a completed async accept request, turing a completed entry into a free one.
    pub fn pop_completed_req(&mut self) -> Option<(u32, A)> {
        let completed_idx = self.completed.pop_front()?;
        let entry = &self.entries[completed_idx];
        let incoming_fd = match entry {
            Completed(fd) => {
                *fd
            },
            _ => unreachable!("the entry should have been completed"),
        };
        let incoming_addr = {
            let AcceptReq { c_addr, c_addr_len } = unsafe { self.reqs.get(completed_index) };
            A:from_c_storage(&c_addr, c_addr_len).unwrap()
        };
        Some((incoming_addr, incoming_fd))
    }
}

impl<A: Addr, R: Runtime> ListenerStream<A, R> {
    pub fn new(prev_state: &Arc<InitStream<A, R>>) -> Result<Self> {
        let common = prev_state.common().clone();
        let addr = prev_state
            .addr()
            .ok_or_else(|| errno!(EINVAL, "socket must be bound to an address before listening"))?;
        let inner = Mutex::new(Inner::new());
        let new_self = Self {
            common,
            addr,
            inner,
        };
        Ok(new_self)
    }
}