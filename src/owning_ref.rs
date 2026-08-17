use core::mem::ManuallyDrop;
use core::ops::{Deref, DerefMut};

#[derive(Debug)]
pub struct OwningRef<'a, T: ?Sized> {
    r: &'a mut ManuallyDrop<T>,
}

impl<'a, T: ?Sized> OwningRef<'a, T> {
    pub unsafe fn from_leaked(r: &'a mut T) -> Self {
        Self {
            r: unsafe { core::mem::transmute::<&'a mut T, &'a mut ManuallyDrop<T>>(r) },
        }
    }

    pub fn leak(self) -> &'a mut T {
        let mut r = ManuallyDrop::new(self);
        unsafe { core::mem::transmute::<&mut ManuallyDrop<T>, &mut T>(r.r) }
    }
}

impl<'a, T> OwningRef<'a, T> {
    pub fn take(self) -> T {
        let mut r = ManuallyDrop::new(self);
        unsafe { ManuallyDrop::take(r.r) }
    }
}

impl<'a, T: ?Sized> Deref for OwningRef<'a, T> {
    type Target = T;
    fn deref(&self) -> &T {
        unsafe { core::mem::transmute::<&ManuallyDrop<T>, &T>(self.r) }
    }
}

impl<'a, T: ?Sized> DerefMut for OwningRef<'a, T> {
    fn deref_mut(&mut self) -> &mut T {
        unsafe { core::mem::transmute::<&mut ManuallyDrop<T>, &mut T>(self.r) }
    }
}

impl<'a, T: ?Sized> Drop for OwningRef<'a, T> {
    fn drop(&mut self) {
        unsafe { ManuallyDrop::drop(self.r) };
    }
}

#[derive(Debug)]
pub struct OwningRefIter<'a, T>(OwningRef<'a, [T]>);

impl<'a, T> IntoIterator for OwningRef<'a, [T]> {
    type Item = T;
    type IntoIter = OwningRefIter<'a, T>;
    fn into_iter(self) -> Self::IntoIter {
        OwningRefIter(self)
    }
}

impl<'a, T> Iterator for OwningRefIter<'a, T> {
    type Item = T;
    fn next(&mut self) -> Option<T> {
        let slice: &mut [ManuallyDrop<T>] = unsafe { core::mem::transmute_copy(&self.0.r) };
        let (head, tail) = slice.split_first_mut()?;
        self.0.r =
            unsafe { core::mem::transmute::<&mut [ManuallyDrop<T>], &mut ManuallyDrop<[T]>>(tail) };
        Some(unsafe { ManuallyDrop::take(head) })
    }
}

pub trait WithOwningRef<T: ?Sized> {
    fn with_owning_ref<R>(self, f: impl for<'a> FnOnce(OwningRef<'a, T>) -> R) -> R;
}

impl<T> WithOwningRef<T> for T {
    fn with_owning_ref<R>(self, f: impl for<'a> FnOnce(OwningRef<'a, T>) -> R) -> R {
        let mut value = ManuallyDrop::new(self);
        f(OwningRef { r: &mut value })
    }
}

impl<T: ?Sized> WithOwningRef<T> for Box<T> {
    fn with_owning_ref<R>(self, f: impl for<'a> FnOnce(OwningRef<'a, T>) -> R) -> R {
        let mut boxed = unsafe { Box::from_raw(Box::into_raw(self) as *mut ManuallyDrop<T>) };
        f(OwningRef { r: &mut *boxed })
    }
}

impl<T> WithOwningRef<[T]> for Vec<T> {
    fn with_owning_ref<R>(self, f: impl for<'a> FnOnce(OwningRef<'a, [T]>) -> R) -> R {
        let (ptr, len, cap) = self.into_raw_parts();
        let mut vec: Vec<ManuallyDrop<T>> = unsafe { Vec::from_raw_parts(ptr.cast(), len, cap) };
        f(OwningRef {
            r: unsafe {
                core::mem::transmute::<&mut [ManuallyDrop<T>], &mut ManuallyDrop<[T]>>(&mut *vec)
            },
        })
    }
}

impl<T, const N: usize> WithOwningRef<[T]> for [T; N] {
    fn with_owning_ref<R>(self, f: impl for<'a> FnOnce(OwningRef<'a, [T]>) -> R) -> R {
        let mut arr = ManuallyDrop::new(self);
        f(OwningRef { r: &mut arr })
    }
}
