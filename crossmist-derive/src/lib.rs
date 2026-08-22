mod entrypoint;
mod func;
mod object;

use proc_macro::TokenStream;

#[proc_macro_attribute]
pub fn entrypoint(meta: TokenStream, input: TokenStream) -> TokenStream {
    entrypoint::entrypoint(meta, input)
}

#[proc_macro]
pub fn func(input: TokenStream) -> TokenStream {
    func::func(input)
}

#[proc_macro_derive(Object)]
pub fn derive_object(input: TokenStream) -> TokenStream {
    object::derive_object(input)
}
