use std::collections::{HashMap, VecDeque};

use async_io::event::{Events, Observer, Pollee, Poller};

use super::{EpollCtl, EpollEntry, EpollEvent, EpollFlags};
use crate::prelude::*;
use crate::util::new_self_ref_arc;

/// A file-like object that provides epoll API.
///
/// Conceptually, we maintain two lists: one consists of all interesting files,
/// which can be managed by the epoll ctl commands; the other are for ready files,
/// which are files that have some events. A epoll wait only needs to iterate the
/// ready list and poll each file to see if the file is ready for the interesting
/// I/O.
///
/// To maintain the ready list, we need to monitor interesting events that happen
/// on the files. To do so, the `EpollFile` registers itself as an `Observer` to
/// the monotored files. Thus, we can add a file to the ready list when an interesting
/// event happens on the file.
pub struct EpollFile {
    // All interesting entries.
    interest: SgxMutex<HashMap<FileDesc, Arc<EpollEntry>>>,
    // Entries that are probably ready (having events happened).
    ready: SgxMutex<VecDeque<Arc<EpollEntry>>>,
    // EpollFile itself is also pollable
    pollee: Pollee,
    // Any EpollFile is wrapped with Arc when created.
    weak_self: Weak<Self>,
}

impl EpollFile {
    /// Creates a new epoll file.
    ///
    /// An `EpollFile` is always contained inside `Arc`.
    pub fn new() -> Arc<Self> {
        let new_self = Self {
            interest: Default::default(),
            ready: Default::default(),
            pollee: Pollee::new(Events::empty()),
            weak_self: Weak::new(),
        };
        new_self_ref_arc!(new_self)
    }

    /// Control the interest list of the epoll file.
    pub fn control(&self, cmd: &EpollCtl) -> Result<()> {
        match *cmd {
            EpollCtl::Add(fd, ep_event, ep_flags) => self.add_interest(fd, ep_event, ep_flags),
            EpollCtl::Del(fd) => self.del_interest(fd),
            EpollCtl::Mod(fd, ep_event, ep_flags) => self.mod_interest(fd, ep_event, ep_flags),
        }
    }

    fn add_interest(&self, fd: FileDesc, ep_event: EpollEvent, ep_flags: EpollFlags) -> Result<()> {
        self.warn_unsupported_flags(&ep_flags);

        let file = current!().file(fd)?;
        let weak_file = FileHandle::downgrade(&file);
        let mask = ep_event.mask;
        let entry = EpollEntry::new(fd, weak_file, ep_event, ep_flags, self.weak_self.clone());

        // Add the new entry to the interest list and start monitering its events
        let mut interest = self.interest.lock().unwrap();
        if interest.contains_key(&fd) {
            return_errno!(EEXIST, "the fd has been added");
        }
        file.register_observer(entry.clone(), Events::all())?;
        interest.insert(fd, entry.clone());
        drop(interest);

        // Add the new entry to the ready list if the file is ready
        let events = file.poll(mask, None);
        if !events.is_empty() {
            self.push_interest(entry);
        }
        Ok(())
    }

    fn del_interest(&self, fd: FileDesc) -> Result<()> {
        let mut interest = self.interest.lock().unwrap();
        let entry = interest
            .remove(&fd)
            .ok_or_else(|| errno!(ENOENT, "fd is not in the interest list"))?;

        // If this epoll entry is in the ready list, then we should delete it.
        // But unfortunately, deleting an entry from the ready list has a
        // complexity of O(N).
        //
        // To optimize the performance, we only mark the epoll entry as
        // deleted at this moment. The real deletion happens when the ready list
        // is scanned in EpolFile::wait.
        entry.set_deleted();

        let file = match entry.file() {
            Some(file) => file,
            // TODO: should we warn about it?
            None => return,
        };
        file.unregister_observer(&(entry as _)).unwrap();
        Ok(())
    }

    fn mod_interest(&self, fd: FileDesc, ep_event: EpollEvent, ep_flags: EpollFlags) -> Result<()> {
        self.warn_unsupported_flags(&flags);

        // Update the epoll entry
        let interest = self.interest.lock().unwrap();
        let entry = interest
            .get(&fd)
            .ok_or_else(|| errno!(ENOENT, "fd is not in the interest list"))?;
        if entry.is_deleted() {
            return_errno!(ENOENT, "fd is not in the interest list");
        }
        let new_mask = ep_event.mask();
        entry.update(ep_event, ep_flags);
        drop(interest);

        // Add the updated entry to the ready list if the file is ready
        let file = match entry.file() {
            Some(file) => file,
            None => return,
        };
        let events = file.poll(mask, None);
        if !events.is_empty() {
            self.push_ready(entry);
        }
    }

    /// Wait for interesting events happen on the files in the interest list
    /// of the epoll file.
    ///
    /// This method blocks until either some interesting events happen or
    /// the timeout expires or a signal arrives. The first case returns
    /// `Ok(events)`, where `events` is a `Vec` containing at most `max_events`
    /// number of `EpollEvent`s. The second and third case returns errors.
    ///
    /// When `max_events` equals to zero, the method returns when the timeout
    /// expires or a signal arrives.
    pub async fn wait(
        &self,
        max_events: usize,
        //timeout: Option<&mut Duration>,
    ) -> Result<Vec<EpollEvent>> {
        let mut ep_events = Vec::new();
        let mut poller = Lazy::new(|| Poller::new(0));
        loop {
            // Try to pop some ready entries
            if self.pop_ready(max_events, &mut ep_events) > 0 {
                return Ok(ready_entries);
            }

            // If no ready entries for now, wait for them
            let is_ready = self.pollee.poll_by(Events::IN, poller.borrow_mut());
            if !is_ready {
                poller.wait().await;
            }
        }
    }

    fn push_ready(&self, entry: Arc<EpollEntry>) {
        let mut ready = self.ready.lock().unwrap();
        if entry.is_ready() || entry.is_deleted() {
            return;
        }
        entry.set_ready();
        ready.push_back(entry);

        self.pollee.add_events(Events::IN);
    }

    fn pop_ready(self, mut max_events: usize, ep_events: &mut Vec<EpollEvent>) -> usize {
        let mut ready = self.ready.lock().unwrap();
        // Pop as many ready entries as possible
        let mut num_events = 0;
        loop {
            let max_events = max_events.max(ready.len());
            if max_events == 0 {
                break;
            }

            ready
                .drain(..max_events)
                .for_each(|entry| entry.reset_ready())
                .filter(|entry| entry.poll().is_empty())
                .for_each(|entry| {
                    ep_events.push(entry.ep_event());
                    num_events += 1;
                    max_events -= 1;
                });
        }
        // Clear the epoll file's events if no ready entries
        if ready.len() == 0 {
            self.pollee.del_events(Events::IN);
        }
        num_events
    }

    fn warn_unsupported_flags(&self, flags: &EpollFlags) {
        if flags.intersects(EpollFlags::EXCLUSIVE | EpollFlags::WAKE_UP) {
            warn!("{:?} contains unsupported flags", flags);
        }
    }
}

impl Observer for EpollEntry {
    fn on_events(&self, _pollee_id: u64, _events: Events) {
        // Fast path
        if self.is_ready() || self.is_deleted() {
            return;
        }

        let epoll_file = self.epoll_file();
        epoll_file.push_ready(self.self_arc());
    }
}
