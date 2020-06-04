use std::cell::UnsafeCell;
use std::marker::PhantomData;

#[derive(Debug)]
pub struct UnsafeCache<'a, T: 'a>{
    cache: Vec<Box<UnsafeCell<T>>>,
    lifetime: PhantomData<&'a T>,
}

impl<'a, T> UnsafeCache<'a, T> {
    pub fn get_mut(&self, index: usize) -> Option<&'a mut T> {
        let value = self.cache.get(index)?;
        // SAFETY: the contained value is guaranteed to never be deallocated until `self` is,
        // since we neither expose method removing values from the `inner`, nor expose any
        // option to mutate the containing Box. The lifetime should be fine, though the
        // real safety issue is this permits multiple mutable references to a given element
        unsafe{ value.get().as_mut() }
    }

    /// Push a new element to the cache and return its index
    pub fn push(&mut self, t: T) -> usize {
        let len = self.cache.len();
        self.cache.push(Box::new(UnsafeCell::new(t)));
        len
    }
}

impl<'a, T> Default for UnsafeCache<'a, T> {
    fn default() -> UnsafeCache<'a, T> {
        UnsafeCache {
            cache: vec![],
            lifetime: PhantomData,
        }
    }
}