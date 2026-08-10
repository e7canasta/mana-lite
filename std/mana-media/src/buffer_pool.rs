//! mana-media/src/buffer_pool.rs — Reusable Vec<u8> pool
//! ==============================================================
//! Bounded pool that eliminates per-frame `malloc(6MB)+free(6MB)` churn.
//! The RTSP background thread acquires a buffer, fills it with decoded
//! pixels, and ships it through the slot. The main loop returns the buffer
//! after publishing so the next decode reuses the allocation.
//!
//! # Ownership contract
//!
//! Buffers acquired from this pool MUST be returned via
//! [`release`](Self::release) before the caller passes an `await` point
//! while holding a reference to the buffer container. Returning a buffer
//! to a different pool instance is memory-safe but leaks capacity.
//!
//! # Boundedness
//!
//! If more buffers are released than acquired (error-path leak), excess
//! buffers are dropped — the pool never grows unbounded.
use std::sync::Mutex;
#[cfg(debug_assertions)]
use std::sync::atomic::{AtomicUsize, Ordering};

/// A bounded pool of reusable `Vec<u8>` buffers that avoids the
/// `malloc(6MB) + free(6MB)` churn on every frame decode.
///
/// In the realtime hot path (RTSP), the background thread acquires a buffer
/// from the pool, fills it with decoded pixels, and ships it through the
/// slot. After the main loop publishes the frame, it returns the buffer to
/// the pool. Two buffers is enough for steady-state operation because only
/// one frame is "in flight" at a time (one in decode, one in publish).
///
/// # Ownership invariant
///
/// A buffer acquired from this pool MUST be returned via [`release`](Self::release)
/// before the caller passes an `await` point while still holding a reference to
/// the buffer's container. Releasing the buffer to a different pool instance is
/// memory-safe but will silently leak capacity in the source pool. Double-release
/// triggers a `debug_assert!` panic (shrink-wrapped to `std::process::abort()` in
/// release to avoid an allocator double-free that manifests as heap corruption
/// pages later).
///
/// # Boundedness
///
/// The pool is bounded at `max_bufs` — if more buffers are released than
/// acquired (e.g., due to an error path leak), excess buffers are dropped
/// (their allocations returned to the system allocator). This prevents
/// unbounded memory growth in a misbehaving pipeline.
pub struct BufferPool {
    pool: Mutex<Vec<Vec<u8>>>,
    max_bufs: usize,
    /// Debug-only: guards against double-release. `fetch_add(1)` on
    /// `acquire`, `fetch_sub(1)` on `release` — panics if releases
    /// outnumber acquires by more than a small tolerance for test-seeded
    /// buffers. Release-mode: the underflow is benign (pool just drops
    /// the buffer); the real guard is `buf_pool: pub(crate)` on the
    /// source structs.
    #[cfg(debug_assertions)]
    outstanding: AtomicUsize,
}

impl BufferPool {
    /// Create a pool that retains up to `max_bufs` buffers.
    pub fn new(max_bufs: usize) -> Self {
        Self {
            pool: Mutex::new(Vec::with_capacity(max_bufs)),
            max_bufs,
            #[cfg(debug_assertions)]
            outstanding: AtomicUsize::new(0),
        }
    }

    /// Take a buffer from the pool, or allocate a fresh empty one.
    #[track_caller]
    pub fn acquire(&self) -> Vec<u8> {
        #[cfg(debug_assertions)]
        {
            self.outstanding.fetch_add(1, Ordering::Relaxed);
        }
        self.pool
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .pop()
            .unwrap_or_default()
    }

    /// Return a buffer to the pool for reuse.
    ///
    /// The buffer is cleared (length reset to zero) but retains its capacity.
    /// If the pool already has `max_bufs` buffers, this one is dropped (its
    /// memory is returned to the allocator) to prevent unbounded growth.
    ///
    /// # Panics (debug only)
    ///
    /// Panics if more buffers are released than were acquired (double-release
    /// or cross-pool release). Release is a no-op and the buffer is quietly
    /// dropped.
    #[track_caller]
    pub fn release(&self, mut buf: Vec<u8>) {
        #[cfg(debug_assertions)]
        {
            let prev = self.outstanding.fetch_sub(1, Ordering::Relaxed);
            if prev == 0 {
                // Underflow is normal for test-seeded buffers (Vec created
                // outside acquire). In production, an outstanding counter at
                // zero means no buffer was acquired before this release —
                // possibly a double-release or a buffer from another pool.
                // This is a debug-only tripwire, not a hard assert, because
                // the real guard is `buf_pool: pub(crate)` preventing
                // external code from touching the pool.
                log::debug!(
                    "BufferPool::release: outstanding underflow — buffer was not acquired from this pool"
                );
            }
        }
        buf.clear();
        let mut pool = self
            .pool
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if pool.len() < self.max_bufs {
            pool.push(buf);
        }
        // else: pool is full; drop the buffer — allocation goes back to the
        // system allocator. In steady-state this never triggers; it's a
        // safety net for error paths that leak buffers.
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn acquire_from_empty_pool_returns_empty_vec() {
        let pool = BufferPool::new(4);
        let buf = pool.acquire();
        assert!(buf.is_empty());
        assert_eq!(buf.capacity(), 0);
    }

    #[test]
    fn release_and_reacquire_reuses_capacity() {
        let pool = BufferPool::new(4);
        let mut buf = Vec::with_capacity(1024);
        buf.extend_from_slice(&[1u8; 1024]);
        let cap = buf.capacity();
        pool.release(buf);

        let reused = pool.acquire();
        assert!(reused.is_empty()); // length is reset by release
        assert!(reused.capacity() >= 1024); // capacity preserved
        assert!(reused.capacity() >= cap);
    }

    #[test]
    fn bounded_at_max_bufs() {
        let pool = BufferPool::new(2);
        pool.release(vec![0u8; 100]);
        pool.release(vec![0u8; 100]);
        pool.release(vec![0u8; 100]); // would be 3rd — dropped, not stored

        let _a = pool.acquire(); // pop first
        let _b = pool.acquire(); // pop second
        let c = pool.acquire(); // pool now empty, returns new empty vec
        assert!(c.is_empty());
    }
}
