mod builtins;

mod closures;
pub use closures::{Closure, FnMutObject, FnObject, FnOnceObject, func};

mod object;
pub use object::Object;

mod relocation;

mod static_fn_ptr;
pub use static_fn_ptr::StaticFnPtr;

mod static_ref;
pub use static_ref::StaticRef;

mod unsized_builtins;
