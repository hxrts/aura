//! Allocation reduction strategies for the effect system
//!
//! This module provides techniques to reduce memory allocations
//! during effect execution, improving performance and reducing GC pressure.

use once_cell::sync::Lazy;
use parking_lot::RwLock;
use std::borrow::Cow;
use std::sync::Arc;
use string_cache::DefaultAtom;

#[cfg(target_arch = "wasm32")]
use fnv::FnvHashMap as HashMap;
#[cfg(not(target_arch = "wasm32"))]
use std::collections::HashMap;

/// Thread-safe string interner for reducing string allocations
pub struct StringInterner {
    cache: RwLock<HashMap<String, DefaultAtom>>,
}

impl StringInterner {
    /// Create a new string interner
    pub fn new() -> Self {
        Self {
            cache: RwLock::new(HashMap::default()),
        }
    }

    /// Intern a string
    pub fn intern(&self, s: &str) -> DefaultAtom {
        // Fast path for common strings
        match s {
            "type" | "id" | "data" | "error" | "result" | "method" | "params" => {
                return DefaultAtom::from(s);
            }
            _ => {}
        }

        // Check cache
        {
            let cache = self.cache.read();
            if let Some(atom) = cache.get(s) {
                return atom.clone();
            }
        }

        // Slow path - add to cache
        let atom = DefaultAtom::from(s);
        self.cache.write().insert(s.to_string(), atom.clone());
        atom
    }

    /// Get statistics about the interner
    pub fn stats(&self) -> InternerStats {
        let cache = self.cache.read();
        InternerStats {
            interned_count: cache.len(),
        }
    }
}

impl Default for StringInterner {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug)]
pub struct InternerStats {
    pub interned_count: usize,
}

/// Global string interner instance
pub static STRING_INTERNER: Lazy<StringInterner> = Lazy::new(StringInterner::new);

/// Intern a string using the global interner
pub fn intern(s: &str) -> DefaultAtom {
    STRING_INTERNER.intern(s)
}

/// Zero-copy string wrapper for avoiding allocations
#[derive(Debug, Clone)]
pub enum ZeroCopyString {
    Static(&'static str),
    Interned(DefaultAtom),
    Owned(String),
}

impl ZeroCopyString {
    /// Create from a static string
    pub const fn from_static(s: &'static str) -> Self {
        Self::Static(s)
    }

    /// Create from an interned string
    pub fn from_interned(s: &str) -> Self {
        Self::Interned(intern(s))
    }

    /// Create from an owned string
    pub fn from_owned(s: String) -> Self {
        Self::Owned(s)
    }

    /// Get as string slice
    pub fn as_str(&self) -> &str {
        match self {
            Self::Static(s) => s,
            Self::Interned(atom) => atom.as_ref(),
            Self::Owned(s) => s.as_str(),
        }
    }
}

impl From<&'static str> for ZeroCopyString {
    fn from(s: &'static str) -> Self {
        Self::Static(s)
    }
}

impl From<String> for ZeroCopyString {
    fn from(s: String) -> Self {
        Self::Owned(s)
    }
}

impl AsRef<str> for ZeroCopyString {
    fn as_ref(&self) -> &str {
        self.as_str()
    }
}

/// Small vector optimization for common cases
#[derive(Debug, Clone)]
pub enum SmallVec<T> {
    Inline([Option<T>; 3]),
    Heap(Vec<T>),
}

impl<T> SmallVec<T> {
    /// Create an empty SmallVec
    pub fn new() -> Self {
        Self::Inline([None, None, None])
    }

    /// Create with capacity
    pub fn with_capacity(capacity: usize) -> Self {
        if capacity <= 3 {
            Self::new()
        } else {
            Self::Heap(Vec::with_capacity(capacity))
        }
    }

    /// Push an element
    pub fn push(&mut self, value: T) {
        match self {
            Self::Inline(arr) => {
                for slot in arr.iter_mut() {
                    if slot.is_none() {
                        *slot = Some(value);
                        return;
                    }
                }
                // Need to spill to heap
                let mut vec = Vec::with_capacity(4);
                for slot in arr.iter_mut() {
                    if let Some(v) = slot.take() {
                        vec.push(v);
                    }
                }
                vec.push(value);
                *self = Self::Heap(vec);
            }
            Self::Heap(vec) => vec.push(value),
        }
    }

    /// Get the length
    pub fn len(&self) -> usize {
        match self {
            Self::Inline(arr) => arr.iter().filter(|s| s.is_some()).count(),
            Self::Heap(vec) => vec.len(),
        }
    }

    /// Check if empty
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Iterate over elements
    pub fn iter(&self) -> SmallVecIter<T> {
        match self {
            Self::Inline(arr) => SmallVecIter::Inline(arr.iter()),
            Self::Heap(vec) => SmallVecIter::Heap(vec.iter()),
        }
    }
}

impl<T> Default for SmallVec<T> {
    fn default() -> Self {
        Self::new()
    }
}

/// Iterator for SmallVec
pub enum SmallVecIter<'a, T> {
    Inline(std::slice::Iter<'a, Option<T>>),
    Heap(std::slice::Iter<'a, T>),
}

impl<'a, T> Iterator for SmallVecIter<'a, T> {
    type Item = &'a T;

    fn next(&mut self) -> Option<Self::Item> {
        match self {
            Self::Inline(iter) => iter.find_map(|opt| opt.as_ref()),
            Self::Heap(iter) => iter.next(),
        }
    }
}

/// Arena allocator for batch allocations
pub struct Arena {
    chunks: RwLock<Vec<Box<[u8]>>>,
    current: RwLock<(usize, usize)>, // (chunk_index, offset)
    chunk_size: usize,
}

impl Arena {
    /// Create a new arena with specified chunk size
    pub fn new(chunk_size: usize) -> Self {
        let initial_chunk = vec![0u8; chunk_size].into_boxed_slice();
        Self {
            chunks: RwLock::new(vec![initial_chunk]),
            current: RwLock::new((0, 0)),
            chunk_size,
        }
    }

    /// Allocate bytes from the arena
    pub fn alloc_bytes(&self, size: usize) -> &[u8] {
        if size > self.chunk_size {
            panic!(
                "Allocation size {} exceeds chunk size {}",
                size, self.chunk_size
            );
        }

        let mut current = self.current.write();
        let chunks = self.chunks.read();

        // Check if current chunk has space
        if current.1 + size > self.chunk_size {
            // Need new chunk
            drop(chunks);
            let mut chunks = self.chunks.write();
            chunks.push(vec![0u8; self.chunk_size].into_boxed_slice());
            *current = (chunks.len() - 1, 0);
            drop(chunks);
        }

        let chunks = self.chunks.read();
        let chunk = &chunks[current.0];
        let offset = current.1;
        current.1 += size;

        // SAFETY: We know the chunk is valid and we have exclusive access to this range
        unsafe {
            let ptr = chunk.as_ptr().add(offset);
            std::slice::from_raw_parts(ptr, size)
        }
    }

    /// Allocate a string from the arena
    pub fn alloc_str(&self, s: &str) -> &str {
        let bytes = self.alloc_bytes(s.len());

        // SAFETY: We have exclusive access to these bytes
        unsafe {
            let ptr = bytes.as_ptr() as *mut u8;
            std::ptr::copy_nonoverlapping(s.as_bytes().as_ptr(), ptr, s.len());
            std::str::from_utf8_unchecked(std::slice::from_raw_parts(ptr, s.len()))
        }
    }

    /// Reset the arena, keeping allocated memory for reuse
    pub fn reset(&self) {
        *self.current.write() = (0, 0);
    }

    /// Get statistics about the arena
    pub fn stats(&self) -> ArenaStats {
        let chunks = self.chunks.read();
        let current = self.current.read();
        ArenaStats {
            chunks: chunks.len(),
            total_capacity: chunks.len() * self.chunk_size,
            used: current.0 * self.chunk_size + current.1,
        }
    }
}

#[derive(Debug)]
pub struct ArenaStats {
    pub chunks: usize,
    pub total_capacity: usize,
    pub used: usize,
}

/// Cow wrapper for reducing clones
pub struct CowWrapper<'a, T: Clone> {
    inner: Cow<'a, T>,
}

impl<'a, T: Clone> CowWrapper<'a, T> {
    /// Create from borrowed data
    pub fn from_borrowed(data: &'a T) -> Self {
        Self {
            inner: Cow::Borrowed(data),
        }
    }

    /// Create from owned data
    pub fn from_owned(data: T) -> Self {
        Self {
            inner: Cow::Owned(data),
        }
    }

    /// Get a reference, avoiding clone if possible
    pub fn as_ref(&self) -> &T {
        self.inner.as_ref()
    }

    /// Convert to owned, cloning only if necessary
    pub fn into_owned(self) -> T {
        self.inner.into_owned()
    }

    /// Make mutable, cloning only if necessary
    pub fn to_mut(&mut self) -> &mut T {
        self.inner.to_mut()
    }
}

/// Reusable buffer pool to avoid repeated allocations
pub struct BufferPool {
    small_buffers: RwLock<Vec<Vec<u8>>>,  // <= 1KB
    medium_buffers: RwLock<Vec<Vec<u8>>>, // <= 64KB
    large_buffers: RwLock<Vec<Vec<u8>>>,  // > 64KB
}

impl BufferPool {
    pub fn new() -> Self {
        Self {
            small_buffers: RwLock::new(Vec::new()),
            medium_buffers: RwLock::new(Vec::new()),
            large_buffers: RwLock::new(Vec::new()),
        }
    }

    /// Get a buffer of at least the specified capacity
    pub fn get_buffer(&self, capacity: usize) -> Vec<u8> {
        if capacity <= 1024 {
            if let Some(mut buf) = self.small_buffers.write().pop() {
                buf.clear();
                return buf;
            }
            Vec::with_capacity(capacity.max(256))
        } else if capacity <= 65536 {
            if let Some(mut buf) = self.medium_buffers.write().pop() {
                buf.clear();
                return buf;
            }
            Vec::with_capacity(capacity.max(4096))
        } else {
            if let Some(mut buf) = self.large_buffers.write().pop() {
                buf.clear();
                return buf;
            }
            Vec::with_capacity(capacity)
        }
    }

    /// Return a buffer to the pool
    pub fn return_buffer(&self, mut buffer: Vec<u8>) {
        let capacity = buffer.capacity();
        buffer.clear();

        if capacity <= 1024 && self.small_buffers.read().len() < 50 {
            self.small_buffers.write().push(buffer);
        } else if capacity <= 65536 && self.medium_buffers.read().len() < 20 {
            self.medium_buffers.write().push(buffer);
        } else if capacity > 65536 && self.large_buffers.read().len() < 5 {
            self.large_buffers.write().push(buffer);
        }
        // Otherwise let it drop
    }
}

impl Default for BufferPool {
    fn default() -> Self {
        Self::new()
    }
}

/// Global buffer pool instance
pub static BUFFER_POOL: Lazy<BufferPool> = Lazy::new(BufferPool::new);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_string_interner() {
        let interner = StringInterner::new();

        let atom1 = interner.intern("hello");
        let atom2 = interner.intern("hello");

        // Should return the same atom
        assert_eq!(atom1, atom2);
        assert_eq!(interner.stats().interned_count, 1);
    }

    #[test]
    fn test_small_vec() {
        let mut vec: SmallVec<i32> = SmallVec::new();

        // Should stay inline
        vec.push(1);
        vec.push(2);
        vec.push(3);
        assert_eq!(vec.len(), 3);

        // Should spill to heap
        vec.push(4);
        assert_eq!(vec.len(), 4);

        let values: Vec<_> = vec.iter().copied().collect();
        assert_eq!(values, vec![1, 2, 3, 4]);
    }

    #[test]
    fn test_arena() {
        let arena = Arena::new(1024);

        let s1 = arena.alloc_str("hello");
        let s2 = arena.alloc_str("world");

        assert_eq!(s1, "hello");
        assert_eq!(s2, "world");

        let stats = arena.stats();
        assert_eq!(stats.chunks, 1);
        assert_eq!(stats.used, 10); // "hello" + "world"
    }

    #[test]
    fn test_buffer_pool() {
        let pool = BufferPool::new();

        let buf1 = pool.get_buffer(512);
        assert!(buf1.capacity() >= 512);

        pool.return_buffer(buf1);

        let buf2 = pool.get_buffer(512);
        assert!(buf2.capacity() >= 512); // Should reuse
    }
}
