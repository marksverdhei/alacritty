//! Synchronization types.
//!
//! Most importantly, a fair mutex is included.

#[cfg(not(target_arch = "wasm32"))]
mod native {
    use parking_lot::{Mutex, MutexGuard};

    /// A fair mutex.
    ///
    /// Uses an extra lock to ensure that if one thread is waiting that it will get
    /// the lock before a single thread can re-lock it.
    pub struct FairMutex<T> {
        /// Data.
        data: Mutex<T>,
        /// Next-to-access.
        next: Mutex<()>,
    }

    impl<T> FairMutex<T> {
        /// Create a new fair mutex.
        pub fn new(data: T) -> FairMutex<T> {
            FairMutex { data: Mutex::new(data), next: Mutex::new(()) }
        }

        /// Acquire a lease to reserve the mutex lock.
        ///
        /// This will prevent others from acquiring a terminal lock, but block if anyone else is
        /// already holding a lease.
        pub fn lease(&self) -> MutexGuard<'_, ()> {
            self.next.lock()
        }

        /// Lock the mutex.
        pub fn lock(&self) -> MutexGuard<'_, T> {
            // Must bind to a temporary or the lock will be freed before going
            // into data.lock().
            let _next = self.next.lock();
            self.data.lock()
        }

        /// Unfairly lock the mutex.
        pub fn lock_unfair(&self) -> MutexGuard<'_, T> {
            self.data.lock()
        }

        /// Unfairly try to lock the mutex.
        pub fn try_lock_unfair(&self) -> Option<MutexGuard<'_, T>> {
            self.data.try_lock()
        }
    }
}

#[cfg(not(target_arch = "wasm32"))]
pub use self::native::FairMutex;

#[cfg(target_arch = "wasm32")]
mod wasm {
    use std::cell::{RefCell, RefMut};

    /// A single-threaded "mutex" for WASM using RefCell.
    ///
    /// On WASM, there is only one thread, so a RefCell suffices.
    pub struct FairMutex<T> {
        data: RefCell<T>,
    }

    impl<T> FairMutex<T> {
        /// Create a new fair mutex.
        pub fn new(data: T) -> FairMutex<T> {
            FairMutex { data: RefCell::new(data) }
        }

        /// Lock the mutex (mutably borrow the data).
        pub fn lock(&self) -> RefMut<'_, T> {
            self.data.borrow_mut()
        }
    }
}

#[cfg(target_arch = "wasm32")]
pub use self::wasm::FairMutex;
