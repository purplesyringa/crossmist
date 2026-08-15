#[macro_use]
extern crate quote;

use proc_macro::TokenStream;
use quote::ToTokens;
use syn::parse_macro_input;
use syn::punctuated::Punctuated;
use syn::spanned::Spanned;
use syn::{DeriveInput, Meta, MetaList};

#[proc_macro_attribute]
pub fn func(meta: TokenStream, input: TokenStream) -> TokenStream {
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

    let fn_args = &input.sig.inputs;

    let mut fn_types = Vec::new();
    let mut arg_names = Vec::new();
    let mut args_from_tuple = Vec::new();
    let mut has_references = false;
    for (i, arg) in fn_args.iter().enumerate() {
        let i = syn::Index::from(i);
        if let syn::FnArg::Typed(pattype) = arg {
            if let syn::Pat::Ident(ref patident) = *pattype.pat {
                let ident = &patident.ident;
                let ty = &pattype.ty;
                fn_types.push(quote! { #ty });
                arg_names.push(quote! { #ident });
                args_from_tuple.push(quote! { args.#i });
                has_references = has_references
                    || matches!(**ty, syn::Type::Reference(_))
                    || matches!(
                        **ty,
                        syn::Type::Group(syn::TypeGroup { ref elem, .. })
                            if matches!(**elem, syn::Type::Reference(_)),
                    );
            } else {
                unreachable!();
            }
        } else {
            unreachable!();
        }
    }

    let return_type_wrapped;
    let pin;
    if tokio_argument.is_some() || smol_argument.is_some() {
        return_type_wrapped = quote! { ::std::pin::Pin<::std::boxed::Box<dyn ::std::future::Future<Output = #return_type>>> };
        pin = quote! { ::std::boxed::Box::pin };
    } else {
        return_type_wrapped = return_type.clone();
        pin = quote! {};
    }

    let entry_code;
    if has_references {
        entry_code = quote! {};
    } else if let Some(arg) = tokio_argument {
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
                Self::invoke(#(#args_from_tuple,)*).await
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
                    Self::invoke(#(#args_from_tuple,)*).await
                })
            }
        };
    } else {
        entry_code = quote! {
            fn entry #generic_params(args: Box<dyn ::core::ops::FnOnce() -> (#(#fn_types,)*)>) -> #return_type {
                let args = args();
                Self::invoke(#(#args_from_tuple,)*)
            }
        };
    }

    let impl_code = if has_references {
        quote! {}
    } else {
        let spawn = quote! { spawn(#type_ident::entry::#generics, (#(#arg_names,)*)) };

        quote! {
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

    let expanded = quote! {
        impl #generic_params ::crossmist::InternalFnOnce<(#(#fn_types,)*)> for #type_ident {
            type Output = #return_type_wrapped;
            fn call_object_once(self, args: (#(#fn_types,)*)) -> Self::Output {
                #pin(#type_ident::invoke(#(#args_from_tuple,)*))
            }
        }
        impl #generic_params ::crossmist::InternalFnMut<(#(#fn_types,)*)> for #type_ident {
            fn call_object_mut(&mut self, args: (#(#fn_types,)*)) -> Self::Output {
                #pin(#type_ident::invoke(#(#args_from_tuple,)*))
            }
        }
        impl #generic_params ::crossmist::InternalFn<(#(#fn_types,)*)> for #type_ident {
            fn call_object(&self, args: (#(#fn_types,)*)) -> Self::Output {
                #pin(#type_ident::invoke(#(#args_from_tuple,)*))
            }
        }

        #[allow(non_camel_case_types)]
        #[derive(::crossmist::Object)]
        #vis struct #type_ident;

        #[allow(unused_mut)]
        impl #type_ident {
            #input

            #entry_code

            // Putting these function in a module named `#ident` would be clearer, but results in
            // scoping issues: `use super::*` imports from the parent module and not the scope, so
            // `#[func]`tions defined inside a block wouldn't compile.
            #impl_code
        }

        #[allow(non_upper_case_globals)]
        #vis const #ident: ::crossmist::CallWrapper<#type_ident> = ::crossmist::CallWrapper(#type_ident);
    };

    TokenStream::from(expanded)
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
                            s.serialize(&self.#ident);
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
                            s.serialize(&self.#i);
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
                    fn serialize_self<'serde>(&'serde self, s: &mut ::crossmist::Serializer<'serde>) {
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
                        let (refs, sers): (Vec<_>, Vec<_>) = fields
                            .named
                            .iter()
                            .map(|field| {
                                let ident = &field.ident;
                                (quote! { ref #ident }, quote! { s.serialize(#ident); })
                            })
                            .unzip();
                        quote! {
                            Self::#ident{ #(#refs,)* } => {
                                s.serialize(&(#i as usize));
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
                    fn serialize_self<'serde>(&'serde self, s: &mut ::crossmist::Serializer<'serde>) {
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
