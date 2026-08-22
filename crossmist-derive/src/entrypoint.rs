use proc_macro::TokenStream;
use quote::{ToTokens, format_ident, quote, quote_spanned};
use syn::{
    FnArg, GenericParam, Index, ItemFn, Meta, MetaList, Pat, ReturnType, Token, Visibility,
    parse_macro_input, punctuated::Punctuated, spanned::Spanned,
};

pub fn entrypoint(meta: TokenStream, input: TokenStream) -> TokenStream {
    let mut tokio_argument = None;
    let mut smol_argument = None;

    let args = parse_macro_input!(meta with Punctuated::<Meta, Token![,]>::parse_terminated);
    for arg in args {
        if arg.path().is_ident("tokio") {
            tokio_argument = Some(arg);
        } else if arg.path().is_ident("smol") {
            smol_argument = Some(arg);
        } else {
            return quote_spanned! { arg.span() => compile_error!("Unknown attribute argument"); }
                .into();
        }
    }

    let mut input = parse_macro_input!(input as ItemFn);

    let return_type = match input.sig.output {
        ReturnType::Default => quote! { () },
        ReturnType::Type(_, ref ty) => quote! { #ty },
    };

    let generic_params = &input.sig.generics;
    let generics = {
        let params: Vec<_> = input
            .sig
            .generics
            .params
            .iter()
            .map(|param| match param {
                GenericParam::Type(ty) => ty.ident.to_token_stream(),
                GenericParam::Lifetime(lt) => lt.lifetime.to_token_stream(),
                GenericParam::Const(con) => con.ident.to_token_stream(),
            })
            .collect();
        quote! { <#(#params,)*> }
    };

    let ident = input.sig.ident;
    input.sig.ident = format_ident!("invoke");

    let vis = input.vis;
    input.vis = Visibility::Public(Default::default());

    let fn_args = &input.sig.inputs;

    let mut fn_types = Vec::new();
    let mut arg_names = Vec::new();
    let mut args_from_tuple = Vec::new();
    for (i, arg) in fn_args.iter().enumerate() {
        let i = Index::from(i);
        if let FnArg::Typed(pattype) = arg {
            if let Pat::Ident(ref patident) = *pattype.pat {
                let ident = &patident.ident;
                let ty = &pattype.ty;
                fn_types.push(quote! { #ty });
                arg_names.push(quote! { #ident });
                args_from_tuple.push(quote! { args.#i });
            } else {
                unreachable!();
            }
        } else {
            unreachable!();
        }
    }

    let entry_code;
    if let Some(arg) = tokio_argument {
        let async_attribute = match arg {
            Meta::Path(_) => quote! { #[tokio::main] },
            Meta::List(MetaList { tokens, .. }) => quote! { #[tokio::main(#tokens)] },
            Meta::NameValue(..) => {
                return quote_spanned! { arg.span() => compile_error!("Invalid syntax for 'tokio' argument"); }.into();
            }
        };
        entry_code = quote! {
            #async_attribute
            async fn entry #generic_params(args: Box<dyn ::core::ops::FnOnce() -> (#(#fn_types,)*)>) -> #return_type {
                // `args` must be deserialized only after the reactor has started.
                let args = args();
                Self::invoke::#generics(#(#args_from_tuple,)*).await
            }
        };
    } else if let Some(arg) = smol_argument {
        if !matches!(arg, Meta::Path(_)) {
            return quote_spanned! { arg.span() => compile_error!("Invalid syntax for 'smol' argument"); }.into();
        }
        entry_code = quote! {
            fn entry #generic_params(args: Box<dyn ::core::ops::FnOnce() -> (#(#fn_types,)*)>) -> #return_type {
                ::crossmist::imp::async_io::block_on(async {
                    let args = args();
                    Self::invoke::#generics(#(#args_from_tuple,)*).await
                })
            }
        };
    } else {
        entry_code = quote! {
            fn entry #generic_params(args: Box<dyn ::core::ops::FnOnce() -> (#(#fn_types,)*)>) -> #return_type {
                let args = args();
                Self::invoke::#generics(#(#args_from_tuple,)*)
            }
        };
    }

    let spawn = quote! { spawn(#ident::entry::#generics, (#(#arg_names,)*)) };

    let expanded = quote! {
        #[allow(non_camel_case_types)]
        #[derive(::crossmist::Object)]
        #vis struct #ident;

        #[allow(unused_mut)]
        impl #ident {
            #input

            #entry_code

            // Putting these function in a module named `#ident` would be clearer, but results in
            // scoping issues: `use super::*` imports from the parent module and not the scope, so
            // `#[entrypoint]`s defined inside a block wouldn't compile.
            pub fn spawn #generic_params(&self, #fn_args) -> ::std::io::Result<::crossmist::Child<#return_type>> {
                unsafe { ::crossmist::blocking::#spawn }
            }
            pub fn run #generic_params(&self, #fn_args) -> ::std::io::Result<#return_type> {
                self.spawn(#(#arg_names,)*)?.join()
            }

            ::crossmist::if_tokio! {
                pub async fn spawn_tokio #generic_params(&self, #fn_args) -> ::std::io::Result<::crossmist::tokio::Child<#return_type>> {
                    unsafe { ::crossmist::tokio::#spawn.await }
                }
                pub async fn run_tokio #generic_params(&self, #fn_args) -> ::std::io::Result<#return_type> {
                    self.spawn_tokio(#(#arg_names,)*).await?.join().await
                }
            }

            ::crossmist::if_smol! {
                pub async fn spawn_smol #generic_params(&self, #fn_args) -> ::std::io::Result<::crossmist::smol::Child<#return_type>> {
                    unsafe { ::crossmist::smol::#spawn.await }
                }
                pub async fn run_smol #generic_params(&self, #fn_args) -> ::std::io::Result<#return_type> {
                    self.spawn_smol(#(#arg_names,)*).await?.join().await
                }
            }
        }
    };

    TokenStream::from(expanded)
}
