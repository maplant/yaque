//! # Yaque: Yet Another QUEue
//!
//! Yaque is yet another disk-backed persistent queue for Rust. It implements
//! an SPSC channel using you OS' filesystem. Its main advantages over a simple
//! `VecDeque<T>` are that
//! * You are not constrained by your RAM size, just by your disk size. This
//! means you can store gigabytes of data without getting OOM killed.
//! * Your data is safe even if you program panics. All the queue state is
//! written to the disk when the queue is dropped.
//! * Your data can *persistence*, that is, can exisit thrhough multiple executions
//! of your program. Think of it as a very rudimentary kind of database.
//! * You can pass data between two processes.
//!
//! Yaque is _assynchronous_ and built directly on top of `mio` and `notify`.
//! It is therefore completely agnostic to the runtime you are using for you
//! application. It will work smoothly with `tokio`, with `async-std` or any
//! other executor of your choice.
//!
//! ## Sample usage
//!
//! To create a new queue, just use the [`channel`] function, passing a
//! directory path on which to mount the queue. If the directiory does not exist
//! on creation, it (and possibly all its parent directories) will be created.
//! ```
//! use yaque::channel;
//!
//! futures::executor::block_on(async {
//!     let (mut sender, mut receiver) = channel("data/my-queue").unwrap();
//! })
//! ```
//! You can also use [`Sender::open`] and [`Receiver::open`] to open only one
//! half of the channel, if you need to.
//!
//! The usage is similar to the MPSC channel in the standard library, except
//! that the receiving method, [`Receiver::recv`] is assynchronous. Writing to
//! the queue with the sender is basically lock-free and atomic.
//! ```
//! use yaque::{channel, try_clear};
//!
//! futures::executor::block_on(async {
//!     // Open using the `channel` function or directly with the constructors.
//!     let (mut sender, mut receiver) = channel("data/my-queue").unwrap();
//!     
//!     // Send stuff with the sender...
//!     sender.send(b"some data").unwrap();
//!
//!     // ... and receive it in the other side.
//!     let data = receiver.recv().await.unwrap();
//!
//!     assert_eq!(&*data, b"some data");
//!
//!     // Call this to make the changes to the queue permanent.
//!     // Not calling it will revert the state of the queue.
//!     data.commit();
//! });
//!
//! // After everything is said and done, you may delete the queue.
//! // Use `clear` for awaiting for the queue to be released.
//! try_clear("data/my-queue").unwrap();
//! ```
//! The returned value `data` is a kind of guard that implements `Deref` and
//! `DerefMut` on the undelying type.
//!
//! ## [`RecvGuard`] and transactional behavior
//!
//! One important thing to notice is that reads from the queue are
//! _transactional_. The `Receiver::recv` returns a [`RecvGuard`] that acts as
//! a _dead man switch_. If dropped, it will revert the dequeue operation,
//! unless [`RecvGuard::commit`] is explicitely called. This ensures that
//! the operation reverts on panics and early returns from errors (such as when
//! using the `?` notation). However, it is necessary to perform one more
//! filesystem oepration while rolling back. During drop, this is done on a
//! "best effort" basis: if an error occurs, it is logged and ignored. This is done
//! because errors cannot propagate outside a drop and panics in drops risk the
//! program being aborted. If you _have_ any clenaup behavior for an error from
//! rolling back, you may call [`RecvGuard::rollback`] which _will_ return the
//! underlying error. 
//!
//! ## Batches
//!
//! You can use the `yaque` queue to send and receive batches of data ,
//! too. The guarantees are the same as with single reads and writes, except
//! that you may save on OS overhead when you send items, since only one disk
//! operation is made. See [`Sender::send_batch`], [`Receiver::recv_batch`] and
//! [`Receiver::recv_while`] for more information on receiver batches.
//!
//! ## `Ctrl+C` and other unexpected events
//! 
//! During some anomalous behavior, the queue might enter an inconsistent state.
//! This inconsistency is mainly related to the position of the sender and of
//! the receiver in the queue. Writting to the queue is an atomic operation.
//! Therefore, unless there is something realy wrong with your OS, you should be
//! fine. 
//! 
//! The queue is (almost) guaranteed to save all the most up-to-date metadata
//! for both receiving and sending parts during a panic. The only exception is
//! if the saving operation fails. However, this is not the case if the process
//! receives a signal from the OS. Signals from the OS are not handled
//! automatically by this library. It is understood that the application
//! programmer knows best how to handle them. If you chose to close queue on
//! `Ctrl+C` or other signals, you are in luck! Saving both sides of the queue
//! is [async-signal-safe](https://man7.org/linux/man-pages/man7/signal-safety.7.html)
//! so you may set up a bare signal hook directly using, for example,
//! [`signal_hook`](https://docs.rs/signal-hook/), if you are the sort of person
//! that enjoys `unsafe` code. If not, there are a ton of completely safe
//! alteratives out there. Choose the one that suits you the best.
//! 
//! Unfortunately, there are times when you get `Aborted` or `Killed`. When this
//! happens, maybe not everything is lost yet. First of all, you will end up
//! with a queue that is locked by no process. If you know that the process
//! owning the locks has indeed past away, you may safely delete the lock files
//! identified by the `.lock` extension. You will also end up with queue
//! metadata pointing to an earlier state in time. Is is easy to guess what the
//! sending metadata should be. Is is the top of the last segment file. However,
//! things get trickier in the receiver side. You know that it is the greatest
//! of two positions:
//! 
//! 1. the bottom of the smallest segment still present in the directory.
//! 
//! 2. the position indicated in the metadata file.
//! 
//! Depending on your usecase, this might be information enough so that not all
//! hope is lost. However, this is all you will get. 
//! 
//! If you really want to err on the safer side, you may use [`Sender::save`]
//! and [`Receiver::save`] to periodically back the queue state up. Just choose
//! you favorite timer implementation and set a simple periodical task up every
//! hundreds of milliseconds. However, be warned that this is only a _mitigation_
//! of consistenciy problems, not a solution. 
//! 
//! ## Known issues and next steps
//!
//! * This is a brand new project. Although I have tested it and it will
//! certainly not implode your computer, don't trust your life on it yet.
//! * Wastes too much kernel time when the queue is small enough and the sender
//! sends many frequent small messages non-atomically. You can mitigate that by
//! writing in batches to the queue.
//! * I intend to make this an MPSC queue in the future.
//! * There are probably unknown bugs hidden in some corner case. If you find
//! one, please fill an issue in GitHub. Pull requests and contributions are
//! also greatly appreciated.

mod state;
mod sync;
mod watcher;

use std::fs::*;
use std::io::{self, Write};
use std::ops::{Deref, DerefMut};
use std::path::{Path, PathBuf};

use state::FilePersistence;
use state::QueueState;
use sync::{FileGuard, TailFollower};

/// The name of segment file in the queue folder.
fn segment_filename<P: AsRef<Path>>(base: P, segment: u64) -> PathBuf {
    base.as_ref().join(format!("{}.q", segment))
}

/// The name of the receiver lock in the queue folder.
fn recv_lock_filename<P: AsRef<Path>>(base: P) -> PathBuf {
    base.as_ref().join("recv.lock")
}

/// Tries to acquire the receiver lock for a queue.
fn try_acquire_recv_lock<P: AsRef<Path>>(base: P) -> io::Result<FileGuard> {
    FileGuard::try_lock(recv_lock_filename(base.as_ref()))?.ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::Other,
            format!(
                "queue `{}` receiver side already in use",
                base.as_ref().to_string_lossy()
            ),
        )
    })
}

/// Acquire the receiver lock for a queue, awaiting if locked.
async fn acquire_recv_lock<P: AsRef<Path>>(base: P) -> io::Result<FileGuard> {
    FileGuard::lock(recv_lock_filename(base.as_ref())).await
}

/// The name of the sender lock in the queue folder.
fn send_lock_filename<P: AsRef<Path>>(base: P) -> PathBuf {
    base.as_ref().join("send.lock")
}

/// Tries to acquire the sender lock for a queue.
fn try_acquire_send_lock<P: AsRef<Path>>(base: P) -> io::Result<FileGuard> {
    FileGuard::try_lock(send_lock_filename(base.as_ref()))?.ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::Other,
            format!(
                "queue `{}` sender side already in use",
                base.as_ref().to_string_lossy()
            ),
        )
    })
}

/// Acquire the sender lock for a queue, awaitn if locked.
async fn acquire_send_lock<P: AsRef<Path>>(base: P) -> io::Result<FileGuard> {
    FileGuard::lock(send_lock_filename(base.as_ref())).await
}

/// The value of a header EOF.
const HEADER_EOF: u32 = std::u32::MAX;

/// The sender part of the queue. This part is lock-free and therefore can be
/// used outside an asynchronous context.
pub struct Sender {
    _file_guard: FileGuard,
    file: io::BufWriter<File>,
    state: QueueState,
    base: PathBuf,
    persistence: FilePersistence,
}

impl Sender {
    /// Opens a queue on a folder indicated by the `base` path for sending. The
    /// folder will be created if it does not already exist.
    ///
    /// # Errors
    ///
    /// This function will return an IO error if the queue is already in use for
    /// sending, which is indicated by a lock file. Also, any other IO error
    /// encountered while opening will be sent.
    pub fn open<P: AsRef<Path>>(base: P) -> io::Result<Sender> {
        // Guarantee that the queue exists:
        create_dir_all(base.as_ref())?;

        log::trace!("created queue directory");

        // Acquire lock and state:
        let file_guard = try_acquire_send_lock(base.as_ref())?;
        let mut persistence = FilePersistence::new();
        let state = persistence.open_send(base.as_ref())?;

        log::trace!("sender lock acquired. Sender state now is {:?}", state);

        // See the docs on OpenOptions::append for why the BufWriter here.
        let file = io::BufWriter::new(
            OpenOptions::new()
                .create(true)
                .append(true)
                .open(segment_filename(base.as_ref(), state.segment))?,
        );

        log::trace!("last segment opened for appending");

        Ok(Sender {
            _file_guard: file_guard,
            file,
            state,
            base: PathBuf::from(base.as_ref()),
            persistence,
        })
    }

    /// Saves the sender queue state. You do not need to use method in most
    /// circumstances, since it is automatically done on drop (yes, it will be
    /// called eve if your thread panics). However, you can use this function to
    ///
    /// 1. Make periodical backups. Use an external timer implementation for this.
    ///
    /// 2. Handle possible IO errors in sending. The `drop` implementation will
    /// ignore (but log) any io errors, which may lead to data loss in an
    /// unreliable filesystem. This happens because no errors are allowed to
    /// propagate on drop and panicking will abort the program if drop is called
    /// during a panic.
    pub fn save(&mut self) -> io::Result<()> {
        self.persistence.save(&self.state)
    }

    /// Just writes to the internal buffer, but doesn't flush it.
    fn write(&mut self, data: &[u8]) -> io::Result<u64> {
        // Get length of the data and write the header:
        let len = data.as_ref().len();
        assert!(len < std::u32::MAX as usize);
        let header = (len as u32).to_be_bytes();

        // Write stuff to the file:
        self.file.write_all(&header)?;
        self.file.write_all(data.as_ref())?;

        Ok(4 + len as u64)
    }

    /// Caps off a segment by writing an EOF header and then moves segment.
    fn cap_off_and_move(&mut self) -> io::Result<()> {
        // Write EOF header:
        self.file.write(&HEADER_EOF.to_be_bytes())?;
        self.file.flush()?;

        // Preserves the already allocaed buffer:
        *self.file.get_mut() = OpenOptions::new()
            .create(true)
            .append(true)
            .open(segment_filename(&self.base, self.state.advance_segment()))?;

        Ok(())
    }

    /// Sends some data into the queue. One send is always atomic.
    ///
    /// # Errors
    ///
    /// This function returns any underlying errors encoutered while writing or
    /// flushing the queue.
    pub fn send<D: AsRef<[u8]>>(&mut self, data: D) -> io::Result<()> {
        // Write to the queue and flush:
        let written = self.write(data.as_ref())?;
        self.file.flush()?; // guarantees atomic operation. See `new`.
        self.state.advance_position(written);

        // See if you are past the end of the file
        if self.state.is_past_end() {
            // If so, create a new file:
            self.cap_off_and_move()?;
        }

        Ok(())
    }

    /// Sends all the contents of an iterable into the queue. All is buffered
    /// to be sent atomically, in one flush operation.
    pub fn send_batch<I>(&mut self, it: I) -> io::Result<()>
    where
        I: IntoIterator,
        I::Item: AsRef<[u8]>,
    {
        let mut written = 0;
        // Drain iterator into the buffer.
        for item in it {
            written += self.write(item.as_ref())?;
        }

        self.file.flush()?; // guarantees atomic operation. See `new`.
        self.state.advance_position(written);

        // See if you are past the end of the file
        if self.state.is_past_end() {
            // If so, create a new file:
            self.cap_off_and_move()?;
        }

        Ok(())
    }
}

impl Drop for Sender {
    fn drop(&mut self) {
        if let Err(err) = self.persistence.save(&self.state) {
            log::error!("could not release sender lock: {}", err);
        }
    }
}

/// The receiver part of the queue. This part is assynchronous and therefore
/// needs an executor that will the poll the futures to completion.
pub struct Receiver {
    _file_guard: FileGuard,
    tail_follower: TailFollower,
    state: QueueState,
    base: PathBuf,
    persistence: FilePersistence,
}

impl Drop for Receiver {
    fn drop(&mut self) {
        if let Err(err) = self.persistence.save(&self.state) {
            log::error!("could not release receiver lock: {}", err);
        }
    }
}

impl Receiver {
    /// Opens a queue for reading. The access will be exclusive, based on the
    /// existence of the temporary file `recv.lock` inside the queue folder.
    ///
    /// # Errors
    ///
    /// This function will return an IO error if the queue is already in use for
    /// receiving, which is indicated by a lock file. Also, any other IO error
    /// encountered while opening will be sent.
    ///
    /// # Panics
    ///
    /// This function will panic if it is not able to set up the notification
    /// handler to watch for file changes.
    pub fn open<P: AsRef<Path>>(base: P) -> io::Result<Receiver> {
        // Guarantee that the queue exists:
        create_dir_all(base.as_ref())?;

        log::trace!("created queue directory");

        // Acquire guarde and state:
        let file_guard = try_acquire_recv_lock(base.as_ref())?;
        let mut persistence = FilePersistence::new();
        let state = persistence.open_recv(base.as_ref())?;

        log::trace!("receiver lock acquired. Receiver state now is {:?}", state);

        // Put the needle on the groove (oh! the 70's):
        let mut tail_follower = TailFollower::open(segment_filename(base.as_ref(), state.segment))?;
        tail_follower.seek(io::SeekFrom::Start(state.position))?;

        log::trace!("last segment opened fo reading");

        Ok(Receiver {
            _file_guard: file_guard,
            tail_follower,
            state,
            base: PathBuf::from(base.as_ref()),
            persistence,
        })
    }

    /// Maybe advance the segment of this receiver.
    async fn advance(&mut self) -> io::Result<()> {
        log::trace!(
            "advancing segment from {} to {}",
            self.state.segment,
            self.state.segment + 1
        );

        // Advance segment and checkpoint!
        self.state.advance_segment();
        if let Err(err) = self.persistence.save(&self.state) {
            log::error!("failed to save receiver: {}", err);
            self.state.retreat_segment();
            return Err(err);
        }

        // Start listening to new segments:
        self.tail_follower = TailFollower::open(segment_filename(&self.base, self.state.segment))?;

        log::trace!("acquired new tail follower");

        // Remove old file:
        remove_file(segment_filename(&self.base, self.state.segment - 1))?;

        log::trace!("removed old segment file");

        Ok(())
    }

    /// Reads one element from the queue, inevitably advancing the file reader.
    async fn read_one(&mut self) -> io::Result<Vec<u8>> {
        // Read the header to get the length:
        let mut header = [0; 4];
        self.tail_follower.read_exact(&mut header).await?;
        let mut len = u32::from_be_bytes(header);

        // If the header is EOF, advance segment:
        if len == HEADER_EOF {
            log::trace!("got EOF header. Advancing...");
            self.advance().await?;

            // Re-read the header:
            log::trace!("re-reading new header from new file");
            self.tail_follower.read_exact(&mut header).await?;
            len = u32::from_be_bytes(header);
        }

        // With the length, read the data:
        let mut data = (0..len).map(|_| 0).collect::<Vec<_>>();
        self.tail_follower
            .read_exact(&mut data)
            .await
            .expect("poisoned queue");

        Ok(data)
    }

    /// Saves the receiver queue state. You do not need to use method in most
    /// circumstances, since it is automatically done on drop (yes, it will be
    /// called eve if your thread panics). However, you can use this function to
    ///
    /// 1. Make periodical backups. Use an external timer implementation for this.
    ///
    /// 2. Handle possible IO errors in sending. The `drop` implementation will
    /// ignore (but log) any io errors, which may lead to data loss in an
    /// unreliable filesystem. This happens because no errors are allowed to
    /// propagate on drop and panicking will abort the program if drop is called
    /// during a panic.
    pub fn save(&mut self) -> io::Result<()> {
        self.persistence.save(&self.state)
    }

    /// Tries to retrieve an element from the queue. The returned value is a
    /// guard that will only commit state changes to the queue when dropped.
    ///
    /// # Panics
    ///
    /// This function will panic if it has to start reading a new segment and
    /// it is not able to set up the notification handler to watch for file
    /// changes.
    pub async fn recv(&mut self) -> io::Result<RecvGuard<'_, Vec<u8>>> {
        let data = self.read_one().await?;

        Ok(RecvGuard {
            receiver: self,
            len: 4 + data.len(),
            item: Some(data),
            set_to_commit: false,
        })
    }

    /// Tries to remove a number of elements from the queue. The returned value
    /// is a guard that will only commit state changes to the queue when dropped.
    ///
    /// # Panics
    ///
    /// This function will panic if it has to start reading a new segment and
    /// it is not able to set up the notification handler to watch for file
    /// changes.
    pub async fn recv_batch(&mut self, n: usize) -> io::Result<RecvGuard<'_, Vec<Vec<u8>>>> {
        let mut data = vec![];

        for _ in 0..n {
            data.push(self.read_one().await?);
        }

        Ok(RecvGuard {
            receiver: self,
            len: data.iter().map(|item| 4 + item.len()).sum(),
            item: Some(data),
            set_to_commit: false,
        })
    }

    /// Tries to a number of elements from the queue. The returned value is a
    /// guard that will only commit state changes to the queue when dropped.
    ///
    /// # Panics
    ///
    /// This function will panic if it has to start reading a new segment and
    /// it is not able to set up the notification handler to watch for file
    /// changes.
    pub async fn recv_while<P: FnMut(&[u8]) -> bool>(
        &mut self,
        mut predicate: P,
    ) -> io::Result<RecvGuard<'_, Vec<Vec<u8>>>> {
        let mut data = vec![];

        // Poor man's do-while
        loop {
            let item = self.read_one().await?;

            if predicate(&item) {
                data.push(item);
            } else {
                break;
            }
        }

        Ok(RecvGuard {
            receiver: self,
            len: data.iter().map(|item| 4 + item.len()).sum(),
            item: Some(data),
            set_to_commit: false,
        })
    }
}

/// A guard that will only log changes on the queue state when dropped.
///
/// If it is dropped in a thread that is panicking, no chage will be logged,
/// allowing for the items to be consumed when a new instance recovers. This
/// allows transactional use of the queue.
///
/// This struct implements `Deref` and `DerefMut`. If you realy, realy want
/// ownership, there is `RecvGuard::into_inner`, but be careful.
pub struct RecvGuard<'a, T> {
    receiver: &'a mut Receiver,
    len: usize,
    item: Option<T>,
    set_to_commit: bool,
}

impl<'a, T> Drop for RecvGuard<'a, T> {
    fn drop(&mut self) {
        // On panic, it is safer if we do *not* do anything.
        // Of course, this makes `RecvGuard` not `UnwindSafe.
        if !self.set_to_commit {
            if let Err(err) = self
                .receiver
                .tail_follower
                .seek(io::SeekFrom::Current(-(self.len as i64)))
            {
                log::error!("unable to rollback on drop: {}", err);
            }
        }
    }
}

impl<'a, T> Deref for RecvGuard<'a, T> {
    type Target = T;
    fn deref(&self) -> &T {
        self.item.as_ref().expect("unreachable")
    }
}

impl<'a, T> DerefMut for RecvGuard<'a, T> {
    fn deref_mut(&mut self) -> &mut T {
        self.item.as_mut().expect("unreachable")
    }
}

impl<'a, T> RecvGuard<'a, T> {
    /// Commits the transaction and returns the underlying value. If you
    /// accedentaly lose this value from now on, it's your own fault!
    pub fn into_inner(mut self) -> T {
        self.item.take().expect("unreachable")
    }

    /// Commits the changes to the queue, consuming this `RecvGuard`.
    pub fn commit(mut self) {
        self.set_to_commit = true;
        drop(self);
    }

    /// Rolls the reader back to the previous point, negating the changes made
    /// on the queue. This is also done on drop. However, on drop, the possible
    /// IO error is ignored (but logged as an error) because we cannot have
    /// errors inside drops. Use this if you want to control errors at rollback.
    ///
    /// # Errors
    ///
    /// If there is some error while moving the reader back, this error will be
    /// return.
    pub fn rollback(mut self) -> io::Result<()> {
        // Paradoxical flag! This is correct. This is *not* a typo.
        self.set_to_commit = true;

        // Do it manually.
        self.receiver
            .tail_follower
            .seek(io::SeekFrom::Current(-(self.len as i64)))
    }
}

/// Convenience function for opening the queue for both sending and receiving.
pub fn channel<P: AsRef<Path>>(base: P) -> io::Result<(Sender, Receiver)> {
    Ok((Sender::open(base.as_ref())?, Receiver::open(base.as_ref())?))
}

/// Tries to deletes a queue at the given path. This function will fail if the
/// queue is in use either for sending or receiving.
pub fn try_clear<P: AsRef<Path>>(base: P) -> io::Result<()> {
    let mut send_lock = try_acquire_send_lock(base.as_ref())?;
    let mut recv_lock = try_acquire_recv_lock(base.as_ref())?;

    // Sets the the locks to ignore when their files magically disappear.
    send_lock.ignore();
    recv_lock.ignore();

    remove_dir_all(base.as_ref())?;

    Ok(())
}

/// Deletes a queue at the given path. This function will await the queue to
/// become available for both sending and receiving.
pub async fn clear<P: AsRef<Path>>(base: P) -> io::Result<()> {
    let mut send_lock = acquire_send_lock(base.as_ref()).await?;
    let mut recv_lock = acquire_recv_lock(base.as_ref()).await?;

    // Sets the the locks to ignore when their files magically disappear.
    send_lock.ignore();
    recv_lock.ignore();

    remove_dir_all(base.as_ref())?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use rand::{Rng, SeedableRng};
    use rand_xorshift::XorShiftRng;
    use std::sync::Arc;

    fn data_lots_of_data() -> impl Iterator<Item = Vec<u8>> {
        let mut rng = XorShiftRng::from_rng(rand::thread_rng()).expect("can init");
        (0..).map(move |_| {
            (0..rng.gen::<usize>() % 128 + 1)
                .map(|_| rng.gen())
                .collect::<Vec<_>>()
        })
    }

    #[test]
    fn create_and_clear() {
        let _ = simple_logger::init_with_level(log::Level::Trace);
        let _ = Sender::open("data/create-and-clear").unwrap();
        try_clear("data/create-and-clear").unwrap();
    }

    #[test]
    #[should_panic]
    fn create_and_clear_fails() {
        let _ = simple_logger::init_with_level(log::Level::Trace);
        let sender = Sender::open("data/create-and-clear-fails").unwrap();
        try_clear("data/create-and-clear-fails").unwrap();
        drop(sender);
    }

    #[test]
    fn create_and_clear_async() {
        let _ = simple_logger::init_with_level(log::Level::Trace);
        let _ = Sender::open("data/create-and-clear-async").unwrap();

        futures::executor::block_on(async { clear("data/create-and-clear-async").await.unwrap() });
    }

    #[test]
    fn test_enqueue() {
        let _ = simple_logger::init_with_level(log::Level::Trace);
        let mut sender = Sender::open("data/enqueue").unwrap();
        for data in data_lots_of_data().take(100_000) {
            sender.send(&data).unwrap();
        }
    }

    /// Test enqueuing everything and then dequeueing everything, with no persistence.
    #[test]
    fn test_enqueue_then_dequeue() {
        let _ = simple_logger::init_with_level(log::Level::Trace);

        // Enqueue:
        let dataset = data_lots_of_data().take(100_000).collect::<Vec<_>>();
        let mut sender = Sender::open("data/enqueue-then-dequeue").unwrap();
        for data in &dataset {
            sender.send(data).unwrap();
        }

        log::trace!("enqueued");

        // Dequeue:
        futures::executor::block_on(async {
            let mut receiver = Receiver::open("data/enqueue-then-dequeue").unwrap();
            let dataset_iter = dataset.iter();
            let mut i = 0u64;

            for should_be in dataset_iter {
                let data = receiver.recv().await.unwrap();
                assert_eq!(&*data, should_be, "at sample {}", i);
                i += 1;
                data.commit();
            }
        });
    }

    /// Test enqueuing and dequeueing, round robin, with no persistenceence.
    #[test]
    fn test_enqueue_and_dequeue() {
        let _ = simple_logger::init_with_level(log::Level::Trace);

        // Enqueue:
        let dataset = data_lots_of_data().take(100_000).collect::<Vec<_>>();
        let mut sender = Sender::open("data/enqueue-and-dequeue").unwrap();

        futures::executor::block_on(async {
            let mut receiver = Receiver::open("data/enqueue-and-dequeue").unwrap();
            let mut i = 0;

            for data in &dataset {
                sender.send(data).unwrap();
                let received = receiver.recv().await.unwrap();
                assert_eq!(&*received, data, "at sample {}", i);

                i += 1;
                received.commit();
            }
        });
    }

    /// Test enqueuing and dequeueing in parallel, with no persistenceence.
    #[test]
    fn test_enqueue_dequeue_parallel() {
        let _ = simple_logger::init_with_level(log::Level::Trace);

        // Generate data:
        let dataset = data_lots_of_data().take(1_000_000).collect::<Vec<_>>();
        let arc_sender = Arc::new(dataset);
        let arc_receiver = arc_sender.clone();

        // Enqueue:
        let enqueue = std::thread::spawn(move || {
            let mut sender = Sender::open("data/enqueue-dequeue-parallel").unwrap();
            for data in &*arc_sender {
                sender.send(data).unwrap();
            }
        });

        // Dequeue:
        let dequeue = std::thread::spawn(move || {
            futures::executor::block_on(async {
                let mut receiver = Receiver::open("data/enqueue-dequeue-parallel").unwrap();
                let dataset_iter = arc_receiver.iter();
                let mut i = 0u64;

                for should_be in dataset_iter {
                    let data = receiver.recv().await.unwrap();
                    assert_eq!(&*data, should_be, "at sample {}", i);
                    i += 1;
                    data.commit();
                }
            });
        });

        enqueue.join().expect("enqueue thread panicked");
        dequeue.join().expect("dequeue thread panicked");
    }
}
