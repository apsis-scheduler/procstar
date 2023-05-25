use std::collections::{BTreeMap, HashSet};
use std::io;
use std::vec::Vec;

use crate::sys::{fd_t, select, FdSet};

//------------------------------------------------------------------------------

pub trait Read {
    fn get_fd(&self) -> fd_t;

    /// Reads from `fd`, when a read is ready.  Returns true if the fd is
    /// complete and should no longer be selected.
    fn read(&mut self) -> bool;
}

pub struct Select<'a> {
    // We use a hash set rather than maintaining an FdSet directly because
    // different OSes have different semantics for how select() modifies its
    // fd_set and whether an fd_set can be copied.
    read_fds: HashSet<fd_t>,

    readers: BTreeMap<fd_t, &'a mut dyn Read>,
}

/// FIXME: Write, error not implememented.
impl<'a> Select<'a> {
    pub fn new() -> Self {
        Select {
            readers: BTreeMap::new(),
            read_fds: HashSet::new(),
        }
    }

    pub fn any(&self) -> bool {
        !self.read_fds.is_empty()
    }

    pub fn insert_reader(&mut self, read: &'a mut dyn Read) {
        let fd = read.get_fd();
        self.read_fds.insert(fd);
        self.readers.insert(fd, read);
    }

    pub fn remove_reader(&mut self, fd: fd_t) -> &'a mut dyn Read {
        self.read_fds.remove(&fd);
        self.readers.remove(&fd).unwrap()
    }

    /// Blocks until a file descriptor is ready, and processes any ready file
    /// descriptors.
    pub fn select(&mut self, timeout: Option<f64>) -> io::Result<()> {
        let mut read_set = FdSet::from_fds(self.read_fds.iter().copied());
        let mut write_set = FdSet::new();
        let mut error_set = FdSet::new();
        select(&mut read_set, &mut write_set, &mut error_set, timeout)?;

        // Process read-ready fds.  Collect those that are done, collect them to
        // avoid inalidating the iter, then remove them.
        self.readers
            .iter_mut()
            .filter_map(|(fd, reader)| {
                if read_set.is_set(*fd) && reader.read() {
                    Some(*fd)
                } else {
                    None
                }
            })
            .collect::<Vec<_>>()
            .into_iter()
            .for_each(|fd| {
                self.remove_reader(fd);
            });

        Ok(())
    }
}
