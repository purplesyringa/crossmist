#![feature(box_patterns)]
#[macro_use]
extern crate quote;

use proc_macro::TokenStream;
use quote::ToTokens;
use syn::parse_macro_input;
use syn::DeriveInput;

#[proc_macro_attribute]
pub fn func(_meta: TokenStream, input: TokenStream) -> TokenStream {
    let mut input = parse_macro_input!(input as syn::ItemFn);

    let tokio_attr_index = input.attrs.iter().position(|attr| {
        let path = &attr.path;
        (quote! {#path}).to_string().contains("tokio :: main")
    });
    let tokio_attr = tokio_attr_index.map(|i| input.attrs.remove(i));

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
        input.sig.ident.to_string(),
        &input as *const syn::ItemFn
    );

    let type_ident = format_ident!("T_{}", link_name);
    let entry_ident = format_ident!("E_{}", link_name);

    let ident = input.sig.ident;
    input.sig.ident = format_ident!("call");

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

    let bound = if args.len() == 0 {
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
    let async_;
    let pin;
    let dot_await;
    if tokio_attr.is_some() {
        return_type_wrapped = quote! { ::std::pin::Pin<::std::boxed::Box<dyn ::std::future::Future<Output = #return_type>>> };
        async_ = quote! { async };
        pin = quote! { Box::pin };
        dot_await = quote! { .await };
    } else {
        return_type_wrapped = return_type.clone();
        async_ = quote! {};
        pin = quote! {};
        dot_await = quote! {};
    }

    let impl_code = if has_references {
        quote! {}
    } else {
        quote! {
            pub unsafe fn spawn_with_flags #generic_params(&self, flags: ::crossmist::subprocess::Flags, #(#fn_args,)*) -> ::std::io::Result<::crossmist::Child<#return_type>> {
                use ::crossmist::BindValue;
                ::crossmist::spawn(::std::boxed::Box::new(::crossmist::CallWrapper(#entry_ident:: #generics ::new(::std::boxed::Box::new(#bound)))), flags)
            }

            #[cfg(feature = "tokio")]
            pub async unsafe fn spawn_with_flags_tokio #generic_params(&self, flags: ::crossmist::subprocess::Flags, #(#fn_args,)*) -> ::std::io::Result<::crossmist::tokio::Child<#return_type>> {
                use ::crossmist::BindValue;
                ::crossmist::tokio::spawn(::std::boxed::Box::new(::crossmist::CallWrapper(#entry_ident:: #generics ::new(::std::boxed::Box::new(#bound)))), flags).await
            }

            pub fn spawn #generic_params(&self, #(#fn_args,)*) -> ::std::io::Result<::crossmist::Child<#return_type>> {
                unsafe { self.spawn_with_flags(::std::default::Default::default(), #(#arg_names,)*) }
            }

            #[cfg(feature = "tokio")]
            pub async fn spawn_tokio #generic_params(&self, #(#fn_args,)*) -> ::std::io::Result<::crossmist::tokio::Child<#return_type>> {
                unsafe { self.spawn_with_flags_tokio(::std::default::Default::default(), #(#arg_names,)*) }.await
            }

            pub fn run #generic_params(&self, #(#fn_args,)*) -> ::std::io::Result<#return_type> {
                self.spawn(#(#arg_names,)*)?.join()
            }

            #[cfg(feature = "tokio")]
            pub async fn run_tokio #generic_params(&self, #(#fn_args,)*) -> ::std::io::Result<#return_type> {
                self.spawn_tokio(#(#arg_names,)*).await?.join().await
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
            #tokio_attr
            #[allow(unreachable_code)] // If func returns !
            #async_ fn call_once(self, args: (::crossmist::handles::RawHandle,)) -> Self::Output {
                let output_tx_handle = args.0;
                use ::crossmist::handles::FromRawHandle;
                let return_value = self.func.deserialize()() #dot_await;
                // Avoid explicitly sending a () result
                if ::crossmist::imp::if_void::<#return_type>().is_none() {
                    // If this function is async, there shouldn't be any tokio task running at this
                    // moment, so it is fine (and more efficient) to use a sync sender
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
            fn call_once(self, args: (#(#fn_types,)*)) -> Self::Output {
                #pin(#type_ident::call(#(#args_from_tuple,)*))
            }
        }
        impl #generic_params ::crossmist::InternalFnMut<(#(#fn_types,)*)> for #type_ident {
            fn call_mut(&mut self, args: (#(#fn_types,)*)) -> Self::Output {
                #pin(#type_ident::call(#(#args_from_tuple,)*))
            }
        }
        impl #generic_params ::crossmist::InternalFn<(#(#fn_types,)*)> for #type_ident {
            fn call(&self, args: (#(#fn_types,)*)) -> Self::Output {
                #pin(#type_ident::call(#(#args_from_tuple,)*))
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

        #[::crossmist::imp::ctor]
        fn crossmist_add_main() {
            *::crossmist::imp::MAIN_ENTRY
                .write()
                .expect("Failed to acquire write access to MAIN_ENTRY") = Some(|| {
                ::crossmist::imp::Report::report(crossmist_old_main())
            });
        }

        fn main() {
            ::crossmist::imp::main()
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
                impl #generics_impl ::crossmist:: NonTrivialObject for #ident #generics #generics_where {
                    fn serialize_self_non_trivial(&self, s: &mut ::crossmist::Serializer) {
                        #(#serialize_fields)*
                    }
                    unsafe fn deserialize_self_non_trivial(d: &mut ::crossmist::Deserializer) -> Self {
                        #deserialize_fields
                    }
                    unsafe fn deserialize_on_heap_non_trivial<'serde>(&self, d: &mut ::crossmist::Deserializer) -> ::std::boxed::Box<dyn ::crossmist:: Object + 'serde> where Self: 'serde {
                        ::std::boxed::Box::new(Self::deserialize_self_non_trivial(d))
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
                                quote! { #ident: unsafe { d.deserialize() } }
                            })
                            .collect();
                        quote! { #i => Self::#ident{ #(#des,)* } }
                    }
                    syn::Fields::Unnamed(fields) => {
                        let des: Vec<_> = (0..fields.unnamed.len())
                            .map(|_| quote! { unsafe { d.deserialize() } })
                            .collect();
                        quote! { #i => Self::#ident(#(#des,)*) }
                    }
                    syn::Fields::Unit => {
                        quote! { #i => Self::#ident }
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
                impl #generics_impl ::crossmist::NonTrivialObject for #ident #generics #generics_where {
                    fn serialize_self_non_trivial(&self, s: &mut ::crossmist::Serializer) {
                        match self {
                            #(#serialize_variants,)*
                        }
                    }
                    unsafe fn deserialize_self_non_trivial(d: &mut ::crossmist::Deserializer) -> Self {
                        match d.deserialize::<usize>() {
                            #(#deserialize_variants,)*
                            _ => panic!("Unexpected enum variant"),
                        }
                    }
                    unsafe fn deserialize_on_heap_non_trivial<'serde>(&self, d: &mut ::crossmist::Deserializer) -> ::std::boxed::Box<dyn ::crossmist::Object + 'serde> where Self: 'serde {
                        ::std::boxed::Box::new(Self::deserialize_self_non_trivial(d))
                    }
                }
                impl #generics_impl ::crossmist::imp::PlainOldData for #ident #generics #generics_where_pod {}
            }
        }
        syn::Data::Union(_) => unimplemented!(),
    };

    TokenStream::from(expanded)
}
