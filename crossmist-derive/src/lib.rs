#![feature(box_patterns)]
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
                syn::GenericParam::Type(ref ty) => ty.ident.to_token_stream(),
                syn::GenericParam::Lifetime(ref lt) => lt.lifetime.to_token_stream(),
                syn::GenericParam::Const(ref con) => con.ident.to_token_stream(),
            })
            .collect();
        quote! { <#(#params,)*> }
    };
    let generic_phantom: Vec<_> = input
        .sig
        .generics
        .params
        .iter()
        .enumerate()
        .map(|(i, param)| {
            let field = format_ident!("f{}", i);
            match param {
                syn::GenericParam::Type(ref ty) => {
                    let ident = &ty.ident;
                    quote! { #field: std::marker::PhantomData<fn(#ident) -> #ident> }
                }
                syn::GenericParam::Lifetime(ref lt) => {
                    let lt = &lt.lifetime;
                    quote! { #field: std::marker::PhantomData<& #lt ()> }
                }
                syn::GenericParam::Const(ref _con) => {
                    unimplemented!()
                }
            }
        })
        .collect();
    let generic_phantom_build: Vec<_> = (0..input.sig.generics.params.len())
        .map(|i| {
            let field = format_ident!("f{}", i);
            quote! { #field: std::marker::PhantomData }
        })
        .collect();

    // Pray all &input are distinct
    let link_name = format!(
        "crossmist_{}_{:?}",
        input.sig.ident, &input as *const syn::ItemFn,
    );

    let type_ident = format_ident!("T_{}", link_name);
    let entry_ident = format_ident!("E_{}", link_name);

    let ident = input.sig.ident;
    input.sig.ident = format_ident!("invoke");

    let vis = input.vis;
    input.vis = syn::Visibility::Public(syn::VisPublic {
        pub_token: <syn::Token![pub] as std::default::Default>::default(),
    });

    let args = &input.sig.inputs;

    let mut fn_args = Vec::new();
    let mut fn_types = Vec::new();
    let mut extracted_args = Vec::new();
    let mut arg_names = Vec::new();
    let mut args_from_tuple = Vec::new();
    let mut binding = Vec::new();
    let mut has_references = false;
    for (i, arg) in args.iter().enumerate() {
        let i = syn::Index::from(i);
        if let syn::FnArg::Typed(pattype) = arg {
            if let syn::Pat::Ident(ref patident) = *pattype.pat {
                let ident = &patident.ident;
                let colon_token = &pattype.colon_token;
                let ty = &pattype.ty;
                fn_args.push(quote! { #ident #colon_token #ty });
                fn_types.push(quote! { #ty });
                extracted_args.push(quote! { crossmist_args.#ident });
                arg_names.push(quote! { #ident });
                args_from_tuple.push(quote! { args.#i });
                binding.push(quote! { .bind_value(#ident) });
                has_references = has_references
                    || matches!(
                        **ty,
                        syn::Type::Reference(_)
                            | syn::Type::Group(syn::TypeGroup {
                                elem: box syn::Type::Reference(_),
                                ..
                            })
                    );
            } else {
                unreachable!();
            }
        } else {
            unreachable!();
        }
    }

    let bound = if args.is_empty() {
        quote! { #ident }
    } else {
        let head_ty = &fn_types[0];
        let tail_ty = &fn_types[1..];
        let head_arg = &arg_names[0];
        let tail_binding = &binding[1..];
        quote! {
            BindValue::<#head_ty, (#(#tail_ty,)*)>::bind_value(::std::boxed::Box::new(#ident), #head_arg) #(#tail_binding)*
        }
    };

    let return_type_wrapped;
    let pin;
    if tokio_argument.is_some() || smol_argument.is_some() {
        return_type_wrapped = quote! { ::std::pin::Pin<::std::boxed::Box<dyn ::std::future::Future<Output = #return_type>>> };
        pin = quote! { ::std::boxed::Box::pin };
    } else {
        return_type_wrapped = return_type.clone();
        pin = quote! {};
    }

    let body;
    if let Some(arg) = tokio_argument {
        let async_attribute = match arg {
            Meta::Path(_) => quote! { #[tokio::main] },
            Meta::List(MetaList { nested, .. }) => quote! { #[tokio::main(#nested)] },
            Meta::NameValue(..) => {
                return quote_spanned! { arg.span() => compile_error!("Invalid syntax for 'tokio' argument"); }.into();
            }
        };
        body = quote! {
            #async_attribute
            async fn body #generic_params (entry: #entry_ident #generics) -> #return_type {
                entry.func.deserialize().expect("Failed to deserialize entry").call_object_box(()).await
            }
        };
    } else if let Some(arg) = smol_argument {
        match arg {
            Meta::Path(_) => {}
            _ => {
                return quote_spanned! { arg.span() => compile_error!("Invalid syntax for 'smol' argument"); }.into();
            }
        }
        body = quote! {
            fn body #generic_params (entry: #entry_ident #generics) -> #return_type {
                ::crossmist::imp::async_io::block_on(entry.func.deserialize().expect("Failed to deserialize entry").call_object_box(()))
            }
        };
    } else {
        body = quote! {
            fn body #generic_params (entry: #entry_ident #generics) -> #return_type {
                entry.func.deserialize().expect("Failed to deserialize entry").call_object_box(())
            }
        };
    }

    let impl_code = if has_references {
        quote! {}
    } else {
        quote! {
            pub fn spawn #generic_params(&self, #(#fn_args,)*) -> ::std::io::Result<::crossmist::Child<#return_type>> {
                use ::crossmist::BindValue;
                unsafe { ::crossmist::blocking::spawn(::std::boxed::Box::new(::crossmist::CallWrapper(#entry_ident:: #generics ::new(::std::boxed::Box::new(#bound))))) }
            }
            pub fn run #generic_params(&self, #(#fn_args,)*) -> ::std::io::Result<#return_type> {
                self.spawn(#(#arg_names,)*)?.join()
            }

            ::crossmist::if_tokio! {
                pub async fn spawn_tokio #generic_params(&self, #(#fn_args,)*) -> ::std::io::Result<::crossmist::tokio::Child<#return_type>> {
                    use ::crossmist::BindValue;
                    unsafe { ::crossmist::tokio::spawn(::std::boxed::Box::new(::crossmist::CallWrapper(#entry_ident:: #generics ::new(::std::boxed::Box::new(#bound))))).await }
                }
                pub async fn run_tokio #generic_params(&self, #(#fn_args,)*) -> ::std::io::Result<#return_type> {
                    self.spawn_tokio(#(#arg_names,)*).await?.join().await
                }
            }

            ::crossmist::if_smol! {
                pub async fn spawn_smol #generic_params(&self, #(#fn_args,)*) -> ::std::io::Result<::crossmist::smol::Child<#return_type>> {
                    use ::crossmist::BindValue;
                    unsafe { ::crossmist::smol::spawn(::std::boxed::Box::new(::crossmist::CallWrapper(#entry_ident:: #generics ::new(::std::boxed::Box::new(#bound))))).await }
                }
                pub async fn run_smol #generic_params(&self, #(#fn_args,)*) -> ::std::io::Result<#return_type> {
                    self.spawn_smol(#(#arg_names,)*).await?.join().await
                }
            }
        }
    };

    let expanded = quote! {
        #[derive(::crossmist::Object)]
        struct #entry_ident #generic_params {
            func: ::crossmist::Delayed<::std::boxed::Box<dyn ::crossmist::FnOnceObject<(), Output = #return_type_wrapped>>>,
            #(#generic_phantom,)*
        }

        impl #generic_params #entry_ident #generics {
            fn new(func: ::std::boxed::Box<dyn ::crossmist::FnOnceObject<(), Output = #return_type_wrapped>>) -> Self {
                Self {
                    func: ::crossmist::Delayed::new(func),
                    #(#generic_phantom_build,)*
                }
            }
        }

        impl #generic_params ::crossmist::InternalFnOnce<(::crossmist::handles::RawHandle,)> for #entry_ident #generics {
            type Output = i32;
            #[allow(unreachable_code, clippy::diverging_sub_expression)] // If func returns !
            fn call_object_once(self, args: (::crossmist::handles::RawHandle,)) -> Self::Output {
                #body
                let return_value = body(self);
                // Avoid explicitly sending a () result
                if ::crossmist::imp::if_void::<#return_type>().is_none() {
                    use ::crossmist::handles::FromRawHandle;
                    // If this function is async, there shouldn't be any task running at this
                    // moment, so it is fine (and more efficient) to use a sync sender
                    let output_tx_handle = args.0;
                    let mut output_tx = unsafe {
                        ::crossmist::Sender::<#return_type>::from_raw_handle(output_tx_handle)
                    };
                    output_tx.send(&return_value)
                        .expect("Failed to send subprocess output");
                }
                0
            }
        }

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

        impl #type_ident {
            #[link_name = #link_name]
            #input

            #impl_code
        }

        #[allow(non_upper_case_globals)]
        #vis const #ident: ::crossmist::CallWrapper<#type_ident> = ::crossmist::CallWrapper(#type_ident);
    };

    TokenStream::from(expanded)
}

#[proc_macro_attribute]
pub fn main(_meta: TokenStream, input: TokenStream) -> TokenStream {
    let mut input = parse_macro_input!(input as syn::ItemFn);

    input.sig.ident = syn::Ident::new("crossmist_old_main", input.sig.ident.span());

    let expanded = quote! {
        #input

        fn main() {
            ::crossmist::init();
            ::std::process::exit(::crossmist::imp::Report::report(crossmist_old_main()));
        }
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
                syn::GenericParam::Type(ref ty) => ty.ident.to_token_stream(),
                syn::GenericParam::Lifetime(ref lt) => lt.lifetime.to_token_stream(),
                syn::GenericParam::Const(ref con) => con.ident.to_token_stream(),
            })
            .collect();
        quote! { <#(#params,)*> }
    };

    let generic_params = &input.generics.params;
    let generics_impl = quote! { <#generic_params> };

    let generics_where = input.generics.where_clause;

    let expanded = match input.data {
        syn::Data::Struct(struct_) => {
            let field_types: Vec<_> = struct_.fields.iter().map(|field| &field.ty).collect();

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
                            #ident: unsafe { d.deserialize() }?,
                        }
                    });
                    quote! { Ok(Self { #(#deserialize_fields)* }) }
                }
                syn::Fields::Unnamed(ref fields) => {
                    let deserialize_fields = fields.unnamed.iter().map(|_| {
                        quote! {
                            unsafe { d.deserialize() }?,
                        }
                    });
                    quote! { Ok(Self (#(#deserialize_fields)*)) }
                }
                syn::Fields::Unit => {
                    quote! { Ok(Self) }
                }
            };

            let generics_where_pod: Vec<_> = match generics_where {
                Some(ref w) => w.predicates.iter().collect(),
                None => Vec::new(),
            };
            let generics_where_pod = quote! {
                where
                    #(#generics_where_pod,)*
                    #(for<'serde> ::crossmist::imp::Identity<'serde, #field_types>: ::crossmist::imp::PlainOldData,)*
            };

            quote! {
                unsafe impl #generics_impl ::crossmist:: NonTrivialObject for #ident #generics #generics_where {
                    fn serialize_self_non_trivial(&self, s: &mut ::crossmist::Serializer) {
                        #(#serialize_fields)*
                    }
                    unsafe fn deserialize_self_non_trivial(d: &mut ::crossmist::Deserializer) -> ::std::io::Result<Self> {
                        #deserialize_fields
                    }
                }
                impl #generics_impl ::crossmist::imp::PlainOldData for #ident #generics #generics_where_pod {}
            }
        }
        syn::Data::Enum(enum_) => {
            let field_types: Vec<_> = enum_
                .variants
                .iter()
                .flat_map(|variant| variant.fields.iter().map(|field| &field.ty))
                .collect();

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
                                quote! { #ident: unsafe { d.deserialize() }? }
                            })
                            .collect();
                        quote! { #i => Ok(Self::#ident{ #(#des,)* }) }
                    }
                    syn::Fields::Unnamed(fields) => {
                        let des: Vec<_> = (0..fields.unnamed.len())
                            .map(|_| quote! { unsafe { d.deserialize() }? })
                            .collect();
                        quote! { #i => Ok(Self::#ident(#(#des,)*)) }
                    }
                    syn::Fields::Unit => {
                        quote! { #i => Ok(Self::#ident) }
                    }
                }
            });

            let generics_where_pod: Vec<_> = match generics_where {
                Some(ref w) => w.predicates.iter().collect(),
                None => Vec::new(),
            };
            let generics_where_pod = quote! {
                where
                    #(#generics_where_pod,)*
                    #(for<'serde> ::crossmist::imp::Identity<'serde, #field_types>: ::crossmist::imp::PlainOldData,)*
            };

            quote! {
                unsafe impl #generics_impl ::crossmist::NonTrivialObject for #ident #generics #generics_where {
                    fn serialize_self_non_trivial(&self, s: &mut ::crossmist::Serializer) {
                        match self {
                            #(#serialize_variants,)*
                        }
                    }
                    unsafe fn deserialize_self_non_trivial(d: &mut ::crossmist::Deserializer) -> ::std::io::Result<Self> {
                        match d.deserialize::<usize>()? {
                            #(#deserialize_variants,)*
                            _ => panic!("Unexpected enum variant"),
                        }
                    }
                }
                impl #generics_impl ::crossmist::imp::PlainOldData for #ident #generics #generics_where_pod {}
            }
        }
        syn::Data::Union(_) => unimplemented!(),
    };

    TokenStream::from(expanded)
}
