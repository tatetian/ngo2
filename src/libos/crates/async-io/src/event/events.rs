bitflags::bitflags! {
    /// Linux-compatible I/O events.
    #[rustfmt::skip]
    pub struct Events: u32 {
        /// = POLLIN
        const IN    = 0x0001;
        /// = POLLPRI
        const PRI   = 0x0002;
        /// = POLLOUT
        const OUT   = 0x0004;
        /// = POLLERR
        const ERR   = 0x0008;
        /// = POLLHUP
        const HUP   = 0x0010;
        /// = POLLNVAL
        const NVAL  = 0x0020;
        /// = POLLRDHUP
        const RDHUP = 0x2000;

        /// Events that are always polled even without specifying them.
        const ALWAYS_POLL = Self::ERR.bits | Self::HUP.bits;
    }
}
