//! FFI value types shared by the AX/CF calls: the CFBase range struct
//! an `AXValue` decodes into, and the owned-reference wrapper every
//! `Copy`/`Create` result comes back through.

use core_foundation::base::{CFRelease, CFTypeRef};

/// `CFRange` from CFBase — declared locally rather than pulling in
/// `core-foundation-sys` for one struct.
#[repr(C)]
#[derive(Clone, Copy)]
pub(super) struct CFRange {
    pub(super) location: isize,
    pub(super) length: isize,
}

/// An owned AX/CF reference — every successful `Copy`/`Create` call
/// hands us a +1 object, and this is the single place it goes back.
pub(super) struct OwnedCF(pub(super) CFTypeRef);

impl Drop for OwnedCF {
    fn drop(&mut self) {
        // Safety: balancing the +1 from the Copy/Create call this
        // wrapper was built from; never constructed from a null.
        unsafe { CFRelease(self.0) }
    }
}
