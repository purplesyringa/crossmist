#[macro_use]
extern crate quote;

use proc_macro::TokenStream;
use quote::ToTokens;
use syn::parse::{Parse, ParseStream};
use syn::parse_macro_input;
use syn::punctuated::Punctuated;
use syn::spanned::Spanned;
use syn::{DeriveInput, Meta, MetaList};

#[proc_macro_attribute]
pub fn func(_meta: TokenStream, input: TokenStream) -> TokenStream {
    let mut input = parse_macro_input!(input as syn::ItemFn);

    let return_type = match input.sig.output {
        syn::ReturnType::Default => quote! { () },
        syn::ReturnType::Type(_, ref ty) => quote! { #ty },
    };

    let generic_params = &input.sig.generics;
    let generics = {
        let params: Vec<_> = input
            .sig
            .generics
            .params
            .iter()
            .map(|param| match param {
                syn::GenericParam::Type(ty) => ty.ident.to_token_stream(),
                syn::GenericParam::Lifetime(lt) => lt.lifetime.to_token_stream(),
                syn::GenericParam::Const(con) => con.ident.to_token_stream(),
            })
            .collect();
        quote! { <#(#params,)*> }
    };

    let type_ident = format_ident!(
        "T_crossmist_{}_{}",
        input.sig.ident,
        format!("{:?}", &input as *const syn::ItemFn), // pray all &input are distinct
    );

    let ident = input.sig.ident;
    input.sig.ident = format_ident!("invoke");

    let vis = input.vis;
    input.vis = syn::Visibility::Public(syn::VisPublic {
        pub_token: <syn::Token![pub] as std::default::Default>::default(),
    });

    let mut fn_types = Vec::new();
    let mut args_from_tuple = Vec::new();
    for (i, arg) in input.sig.inputs.iter().enumerate() {
        let i = syn::Index::from(i);
        if let syn::FnArg::Typed(pattype) = arg {
            let ty = &pattype.ty;
            fn_types.push(quote! { #ty });
            args_from_tuple.push(quote! { args.#i });
        } else {
            unreachable!();
        }
    }

    let return_type_wrapped;
    let pin;
    if input.sig.asyncness.is_some() {
        return_type_wrapped = quote! { ::std::pin::Pin<::std::boxed::Box<dyn ::std::future::Future<Output = #return_type>>> };
        pin = quote! { ::std::boxed::Box::pin };
    } else {
        return_type_wrapped = return_type.clone();
        pin = quote! {};
    }

    let expanded = quote! {
        #[allow(non_camel_case_types)]
        #[derive(::crossmist::Object)]
        #vis struct #type_ident;

        impl #type_ident {
            #input
        }

        impl #generic_params ::crossmist::FnItem<(#(#fn_types,)*)> for #type_ident {
            type Output = #return_type_wrapped;
            fn call(&self, args: (#(#fn_types,)*)) -> Self::Output {
                #pin(Self::invoke::#generics(#(#args_from_tuple,)*))
            }
        }

        #[allow(non_upper_case_globals)]
        #vis const #ident: ::crossmist::CallWrapper<#type_ident> = ::crossmist::CallWrapper(#type_ident);
    };

    TokenStream::from(expanded)
}

#[proc_macro_attribute]
pub fn entrypoint(meta: TokenStream, input: TokenStream) -> TokenStream {
    let mut tokio_argument = None;
    let mut smol_argument = None;

    let args = parse_macro_input!(meta with Punctuated::<Meta, syn::Token![,]>::parse_terminated);
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

    let mut input = parse_macro_input!(input as syn::ItemFn);

    let return_type = match input.sig.output {
        syn::ReturnType::Default => quote! { () },
        syn::ReturnType::Type(_, ref ty) => quote! { #ty },
    };

    let generic_params = &input.sig.generics;
    let generics = {
        let params: Vec<_> = input
            .sig
            .generics
            .params
            .iter()
            .map(|param| match param {
                syn::GenericParam::Type(ty) => ty.ident.to_token_stream(),
                syn::GenericParam::Lifetime(lt) => lt.lifetime.to_token_stream(),
                syn::GenericParam::Const(con) => con.ident.to_token_stream(),
            })
            .collect();
        quote! { <#(#params,)*> }
    };

    let ident = input.sig.ident;
    input.sig.ident = format_ident!("invoke");

    let vis = input.vis;
    input.vis = syn::Visibility::Public(syn::VisPublic {
        pub_token: <syn::Token![pub] as std::default::Default>::default(),
    });

    let fn_args = &input.sig.inputs;

    let mut fn_types = Vec::new();
    let mut arg_names = Vec::new();
    let mut args_from_tuple = Vec::new();
    for (i, arg) in fn_args.iter().enumerate() {
        let i = syn::Index::from(i);
        if let syn::FnArg::Typed(pattype) = arg {
            if let syn::Pat::Ident(ref patident) = *pattype.pat {
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
            Meta::List(MetaList { nested, .. }) => quote! { #[tokio::main(#nested)] },
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

struct ExplicitlyCapturingClosure {
    captures: Option<ExplicitCaptures>,
    closure: syn::ExprClosure,
}

#[allow(dead_code)]
struct ExplicitCaptures {
    move_token: syn::Token![move],
    paren_token: syn::token::Paren,
    captures: Punctuated<ExplicitCapture, syn::Token![,]>,
}

struct ExplicitCapture {
    by_ref: Option<syn::Token![ref]>,
    mutability: Option<syn::Token![mut]>,
    ident: syn::Ident,
    #[allow(unused)]
    colon_token: syn::Token![:],
    ty: syn::Type,
}

impl Parse for ExplicitlyCapturingClosure {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        let mut captures = None;
        if input.peek(syn::Token![move]) {
            captures = Some(input.parse()?);
        }
        let closure = input.parse()?;
        Ok(Self { captures, closure })
    }
}

impl Parse for ExplicitCaptures {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        let captures;
        Ok(Self {
            move_token: input.parse()?,
            paren_token: syn::parenthesized!(captures in input),
            captures: captures.parse_terminated(ExplicitCapture::parse)?,
        })
    }
}

impl Parse for ExplicitCapture {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        let by_ref: Option<syn::Token![ref]> = input.parse()?;
        let mutability = if by_ref.is_some() {
            input.parse()?
        } else {
            None
        };
        Ok(Self {
            by_ref,
            mutability,
            ident: input.parse()?,
            colon_token: input.parse()?,
            ty: input.parse()?,
        })
    }
}

#[proc_macro]
pub fn lambda(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as ExplicitlyCapturingClosure);
    let has_captures = input.captures.is_some();

    let mut captured_by_value_idents = Vec::new();
    let mut captured_by_value_types = Vec::new();
    let mut captured_by_ref_idents = Vec::new();
    let mut captured_by_ref_types = Vec::new();
    let mut captured_by_ref_mut_idents = Vec::new();
    let mut captured_by_ref_mut_types = Vec::new();
    if let Some(ExplicitCaptures { captures, .. }) = input.captures {
        for capture in captures {
            let (idents, types) = if capture.by_ref.is_none() {
                (&mut captured_by_value_idents, &mut captured_by_value_types)
            } else if capture.mutability.is_none() {
                (&mut captured_by_ref_idents, &mut captured_by_ref_types)
            } else {
                (
                    &mut captured_by_ref_mut_idents,
                    &mut captured_by_ref_mut_types,
                )
            };
            idents.push(capture.ident);
            types.push(capture.ty);
        }
    }

    let inputs = input.closure.inputs;
    let output = input.closure.output;
    let body = input.closure.body;

    if !has_captures {
        return quote! {
            {
                #[::crossmist::func]
                fn _unnamed(#inputs) #output {
                    // create a closure for `unused_braces` lint to work correctly
                    (move || #body)()
                }
                Box::new(_unnamed)
            }
        }
        .into();
    }

    quote! {
        {
            #[::crossmist::func]
            fn _unnamed(
                _by_value: (#(#captured_by_value_types,)*),
                _by_ref: &(#(#captured_by_ref_types,)*),
                _by_ref_mut: &mut (#(#captured_by_ref_mut_types,)*),
                #inputs
            ) #output {
                // TODO: inline into the signature once `#[crossmist::func]` supports that
                let (#(#captured_by_value_idents,)*) = _by_value;
                let (#(#captured_by_ref_idents,)*) = _by_ref;
                let (#(#captured_by_ref_mut_idents,)*) = _by_ref_mut;
                // create a closure for `unused_braces` lint to work correctly
                (move || #body)()
            }
            ::std::boxed::Box::new(
                ::crossmist::Bound {
                    func: _unnamed,
                    by_value: (#(#captured_by_value_idents,)*),
                    by_ref: (#(#captured_by_ref_idents,)*),
                    by_ref_mut: (#(#captured_by_ref_mut_idents,)*),
                },
            )
        }
    }
    .into()
}

#[proc_macro_derive(Object)]
pub fn derive_object(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);

    let ident = &input.ident;

    let generics = {
        let params: Vec<_> = input
            .generics
            .params
            .iter()
            .map(|param| match param {
                syn::GenericParam::Type(ty) => ty.ident.to_token_stream(),
                syn::GenericParam::Lifetime(lt) => lt.lifetime.to_token_stream(),
                syn::GenericParam::Const(con) => con.ident.to_token_stream(),
            })
            .collect();
        quote! { <#(#params,)*> }
    };

    let generic_params = &input.generics.params;
    let generics_impl = quote! { <#generic_params> };

    let generics_where = input.generics.where_clause;

    let expanded = match input.data {
        syn::Data::Struct(struct_) => {
            let serialize_fields = match struct_.fields {
                syn::Fields::Named(ref fields) => fields
                    .named
                    .iter()
                    .map(|field| {
                        let ident = &field.ident;
                        quote! {
                            s.serialize(self.#ident);
                        }
                    })
                    .collect(),
                syn::Fields::Unnamed(ref fields) => fields
                    .unnamed
                    .iter()
                    .enumerate()
                    .map(|(i, _)| {
                        let i = syn::Index::from(i);
                        quote! {
                            s.serialize(self.#i);
                        }
                    })
                    .collect(),
                syn::Fields::Unit => Vec::new(),
            };

            let deserialize_fields = match struct_.fields {
                syn::Fields::Named(ref fields) => {
                    let deserialize_fields = fields.named.iter().map(|field| {
                        let ident = &field.ident;
                        quote! {
                            #ident: unsafe { d.deserialize() },
                        }
                    });
                    quote! { Self { #(#deserialize_fields)* } }
                }
                syn::Fields::Unnamed(ref fields) => {
                    let deserialize_fields = fields.unnamed.iter().map(|_| {
                        quote! {
                            unsafe { d.deserialize() },
                        }
                    });
                    quote! { Self (#(#deserialize_fields)*) }
                }
                syn::Fields::Unit => {
                    quote! { Self }
                }
            };

            quote! {
                unsafe impl #generics_impl ::crossmist::Object for #ident #generics #generics_where {
                    fn serialize_self(self, s: &mut ::crossmist::Serializer) {
                        #(#serialize_fields)*
                    }
                    unsafe fn deserialize_self(d: &mut ::crossmist::Deserializer) -> Self {
                        #deserialize_fields
                    }
                }
            }
        }
        syn::Data::Enum(enum_) => {
            let serialize_variants = enum_.variants.iter().enumerate().map(|(i, variant)| {
                let ident = &variant.ident;
                match &variant.fields {
                    syn::Fields::Named(fields) => {
                        let (names, sers): (Vec<_>, Vec<_>) = fields
                            .named
                            .iter()
                            .map(|field| {
                                let ident = &field.ident;
                                (quote! { #ident }, quote! { s.serialize(#ident); })
                            })
                            .unzip();
                        quote! {
                            Self::#ident{ #(#names,)* } => {
                                s.serialize(#i as usize);
                                #(#sers)*
                            }
                        }
                    }
                    syn::Fields::Unnamed(fields) => {
                        let (refs, sers): (Vec<_>, Vec<_>) = (0..fields.unnamed.len())
                            .map(|i| {
                                let ident = format_ident!("a{}", i);
                                (quote! { ref #ident }, quote! { s.serialize(#ident); })
                            })
                            .unzip();
                        quote! {
                            Self::#ident(#(#refs,)*) => {
                                s.serialize(&(#i as usize));
                                #(#sers)*
                            }
                        }
                    }
                    syn::Fields::Unit => {
                        quote! {
                            Self::#ident => {
                                s.serialize(&(#i as usize));
                            }
                        }
                    }
                }
            });

            let deserialize_variants = enum_.variants.iter().enumerate().map(|(i, variant)| {
                let ident = &variant.ident;

                match &variant.fields {
                    syn::Fields::Named(fields) => {
                        let des: Vec<_> = fields
                            .named
                            .iter()
                            .map(|field| {
                                let ident = &field.ident;
                                quote! { #ident: unsafe { d.deserialize() } }
                            })
                            .collect();
                        quote! { #i => Ok(Self::#ident{ #(#des,)* }) }
                    }
                    syn::Fields::Unnamed(fields) => {
                        let des: Vec<_> = (0..fields.unnamed.len())
                            .map(|_| quote! { unsafe { d.deserialize() } })
                            .collect();
                        quote! { #i => Ok(Self::#ident(#(#des,)*)) }
                    }
                    syn::Fields::Unit => {
                        quote! { #i => Ok(Self::#ident) }
                    }
                }
            });

            quote! {
                unsafe impl #generics_impl ::crossmist::Object for #ident #generics #generics_where {
                    fn serialize_self(self, s: &mut ::crossmist::Serializer) {
                        match self {
                            #(#serialize_variants,)*
                        }
                    }
                    unsafe fn deserialize_self(d: &mut ::crossmist::Deserializer) -> ::std::io::Result<Self> {
                        match d.deserialize::<usize>() {
                            #(#deserialize_variants,)*
                            _ => panic!("Unexpected enum variant"),
                        }
                    }
                }
            }
        }
        syn::Data::Union(_) => unimplemented!(),
    };

    TokenStream::from(expanded)
}
