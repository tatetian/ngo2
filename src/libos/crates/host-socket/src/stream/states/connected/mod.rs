
mod receiver;
mod sender;

pub struct ConnectedStream<A: Addr, R: Runtime>  {
    common: Arc<Common>,
    peer_addr: A,
    sender: Sender,
    receiver: Receiver,
}

impl<A: Addr, R: Runtime> ConnectedStream<A, R> {
    pub fn new(common: Arc<Common>, peer_addr: A) -> Result<Arc<Self>> {
        let sender = Sender::new();
        let receiver = Receiver::new();
        let new_self = Self {
            common,
            peer_addr,
            sender,
            receiver,
        };
        Ok(new_self)
    }

    pub fn with_fd_and_peer(fd: u32, peer_addr: A) -> Result<Arc<Self>> {
        let common = Common::with_fd(fd);
        common.pollee.add(Events::IN);
        Self::new(common, peer_addr)
    }

    // TODO: implement other methods

    // Other methods are implemented in the sender and receiver modules
}

impl<A: Addr, R: Runtime> TryFrom<&Arc<ConnectingStream<A, R>>> for ConnectedStream<A, R> {
    type Error = errno::Error;

    fn from(prev_state: &Arc<ConnectingStream<A, R>>) -> Result<Arc<Self>> {
        let common = prev_state.common.clone();
        let peer_addr = prev_state.peer_addr.clone();
        Self::new(common, peer_addr)
    }
}