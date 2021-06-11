

pub struct Stream<A: Addr, R: Runtime> {
    state: RwLock<State<A, R>>,
}

enum State<A: Addr, R: Runtime> {
    // Start state
    Init(Arc<InitStream<A, R>>),
    // Intermediate state
    Connect(Arc<ConnectingStream<A, R>>),
    // Final state 1
    Connected(Arc<ConnectedStream<A, R>>),
    // Final state 2
    Listen(Arc<ListenerStream<A, R>>),
}

impl<A: Addr, R: Runtime> Stream<A, R> {
    pub fn new() -> Self {
        let init_stream = InitStream::new()?;
        let init_state = State::Init(init_stream);
        Self {
            state: RwLock::new(init_state),
        }
    }

    fn new_connected() -> Self {

    }

    pub fn bind(&self, addr: &A) -> Result<()> {
        let state = self.state.read();
        match *state {
            Init(init_stream) => {
                init_stream.bind(addr)
            }
            _ => {
                return_errno!(EINVAL, "");
            }
        }
    }

    pub fn listen(&self, backlog: u32) -> Result<()> {
        let mut state = self.state.write();
        match *state {
            Init(init_stream) => {
                let common = init_stream.common().clone();
                let listener = ListenerStream::new(backlog, common)?;
                *state = State::Listen(listener);
                Ok(())
            }
            _ => {
                return_errno!(EINVAL, "");
            }
        }
    }

    pub async fn connect(&self, addr: &A) -> Result<()> {
        // Create the new intermediate state of connecting and save the
        // old state of init in case of failure to connect.
        let (init_stream, connecting_stream) = {
            let mut state = self.state.write();
            match *state {
                Init(init_stream) => {
                    let common = init_stream.common().clone();
                    let connecting_stream = ConnectingStream::new(addr, common)?;
                    *state = State::Connect(connecting_stream.clone());
                    (init_stream, connecting_stream)
                }
                _ => {
                    return_errno!(EINVAL, "");
                }
            }
        };

        let res = connecting_stream.connect().await;

        // If success, then the state transits to connected; otherwise,
        // the state is restored to the init state.
        match &res {
            Ok(()) => {
                let common = init_stream.common().clone();
                let mut state = self.state.write();
                let connected_stream = ConnectedStream::new(common);
                *state = State::Connected(connected_stream);
            }
            Err(e) => {
                let mut state = self.state.write();
                *state = State::Init(init_stream);
            }
        }
        res
    }

    pub async fn accept(&self) -> Result<Self> {
        let listener_stream = {
            let state = self.state.read();
            match *state {
                Listen(listener_stream) => listener_stream.clone(),
                _ => {
                    return_errno!(EINVAL, "");
                }
            }
        };

        listener_stream.accept().await
    }

    pub async fn read(&self, buf: &mut [u8]) -> Result<usize> {
        let connected_stream = {
            let state = self.state.read();
            match *state {
                Connected(connected_stream) => connected_stream.clone(),
                _ => {
                    return_errno!(EINVAL, "");
                }
            }
        };

        connected_stream.read(buf).await
    }

    pub async fn write(&self, buf: &[u8]) -> Result<usize> {
        let connected_stream = {
            let state = self.state.read();
            match *state {
                Connected(connected_stream) => connected_stream.clone(),
                _ => {
                    return_errno!(EINVAL, "");
                }
            }
        };

        connected_stream.write(buf).await
    }

    pub async fn shutdown(&self, shutdown: Shutdown) -> Result<()> {
        let connected_stream = {
            let state = self.state.read();
            match *state {
                Connected(connected_stream) => connected_stream.clone(),
                _ => {
                    return_errno!(EINVAL, "");
                }
            }
        };
        
        connected_stream.shutdown(shutdown)
    }

    pub fn ioctl(&self, ioctl: &mut dyn IoctlCmd) -> Result<()> {
        return_errno!(EINVAL, "")
    }
    
    pub fn poll_by(&self, mask: Events, mut poller: Option<&mut Poller>) -> Events {
        let state = self.state.read();
        match *state {
            Init(init_stream) => init_stream.poll_by(mask, poller),
            Connect(connect_stream) => connect_stream.poll_by(mask, poller),
            Connected(connected_stream) = connected_stream.poll_by(mask, poller),
            Listen(listener_stream) = listener_stream.poll_by(mask, poller),
        }
    }

    pub fn addr(&self) -> Option<A> {
        let state = self.state.read();
        match *state {
            Init(init_stream) => init_stream.addr(),
            Connect(connect_stream) => connect_stream.addr(),
            Connected(connected_stream) = connected_stream.addr(),
            Listen(listener_stream) = listener_stream.addr(),
        }
    }

    pub fn peer_addr(&self) -> Option<A> {
        let state = self.state.read();
        match *state {
            Connected(connected_stream) = connected_stream.peer_addr(),
            _ => {
                return_errno!(EINVAL, "")
            }
        }
    }
}