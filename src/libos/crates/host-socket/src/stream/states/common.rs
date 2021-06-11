
pub struct Common<A: Addr, R: Runtime> {
    fd: u32,
    pollee: Pollee,
    phantom_data: std::marker::PhantomData<(A, R)>,
}

impl<A: Addr, R: Runtime> Common<A, R> {
    pub fn new() -> Self {
        let domain_c = A::domain() as libc::c_int;
        let fd = unsafe { libc::socket(domain_c, libc::SOCK_STREAM, 0) };
        assert!(fd >= 0);
        let pollee = Pollee::new(Events::empty());
        Self {
            fd: fd as _,
            pollee,
            phantom_data: std::marker::PhantomData,
        }
    }

    pub fn with_fd(fd: u32) -> Self {
        let pollee = Pollee::new(Events::empty());
        Self {
            fd:
            pollee,
            phantom_data: std::marker::PhantomData,
        }
    }

    pub fn io_uring(&self) -> R::IoUring {
        R::io_uring()
    }

    pub fn fd(&self) -> u32 {
        self.fd
    }

    pub fn pollee(&self) -> &Pollee {
        &self.pollee
    }
}