#![feature(box_patterns)]
#[macro_use]
extern crate quote;

use proc_macro::TokenStream;
use quote::ToTokens;
use syn::parse_macro_input;
use syn::DeriveInput;

/// Enable a function to be used as an entrypoint of a child process, and turn it into an
/// [`Object`].
///
/// This macro applies to `fn` functions, including generic ones. It turns the function into an
/// object that can be called (providing the same behavior as if `#[func]` was not used), but also
/// adds various methods for spawning a child process from this function.
///
/// For a function declared as
///
/// ```ignore
/// #[func]
/// fn example(arg1: Type1, ...) -> Output;
/// ```
///
/// ...the methods are:
///
/// ```ignore
/// pub fn spawn(&mut self, arg1: Type1, ...) -> std::io::Result<multiprocessing::Child<Output>>;
/// pub fn run(arg1: Type1, ...) -> std::io::Result<Output>;
/// ```
///
/// For example:
///
/// ```rust
/// use multiprocessing::{func, main};
///
/// #[func]
/// fn example(a: i32, b: i32) -> i32 {
///     a + b
/// }
///
/// #[main]
/// fn main() {
///     assert_eq!(example(5, 7), 12);
///     assert_eq!(example.spawn(5, 7).unwrap().join(), Ok(12));
///     assert_eq!(example.run(5, 7), Ok(12));
/// }
/// ```
///
///
/// ## Asynchronous case
///
/// This section applies if the `tokio` feature is enabled.
///
/// The following methods are also made available:
///
/// ```ignore
/// pub async fn spawn_tokio(&mut self, arg1: Type1, ...) ->
///     std::io::Result<multiprocessing::tokio::Child<Output>>;
/// pub async fn run_tokio(arg1: Type1, ...) -> std::io::Result<Output>;
/// ```
///
/// Additionally, the function may be `async`. In this case, you have to add the `#[tokio::main]`
/// attribute *after* `#[func]`. For instance:
///
/// ```ignore
/// #[multiprocessing::func]
/// #[tokio::main]
/// async fn example() {}
/// ```
///
/// You may pass operands to `tokio::main` just like usual:
///
/// ```rust
/// #[multiprocessing::func]
/// #[tokio::main(flavor = "current_thread")]
/// async fn example() {}
/// ```
///
/// Notice that the use of `spawn` vs `spawn_tokio` is orthogonal to whether the function is
/// `async`: you can start a synchronous function in a child process asynchronously, or vice versa:
///
/// ```rust
/// use multiprocessing::{func, main};
///
/// #[func]
/// fn example(a: i32, b: i32) -> i32 {
///     a + b
/// }
///
/// #[main]
/// #[tokio::main]
/// async fn main() {
///     assert_eq!(example(5, 7), 12);
///     assert_eq!(example.run_tokio(5, 7).await, Ok(12));
/// }
/// ```
///
/// ```rust
/// use multiprocessing::{func, main};
///
/// #[func]
/// #[tokio::main]
/// async fn example(a: i32, b: i32) -> i32 {
///     a + b
/// }
///
/// #[main]
/// fn main() {
///     assert_eq!(example.run(5, 7), Ok(12));
/// }
/// ```
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
        "multiprocessing_{}_{:?}",
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
                extracted_args.push(quote! { multiprocessing_args.#ident });
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
    let ns_tokio;
    if tokio_attr.is_some() {
        return_type_wrapped = quote! { ::std::pin::Pin<::std::boxed::Box<dyn ::std::future::Future<Output = #return_type>>> };
        async_ = quote! { async };
        pin = quote! { Box::pin };
        dot_await = quote! { .await };
        ns_tokio = quote! { ::tokio };
    } else {
        return_type_wrapped = return_type.clone();
        async_ = quote! {};
        pin = quote! {};
        dot_await = quote! {};
        ns_tokio = quote! {};
    }

    let impl_code = if has_references {
        quote! {}
    } else {
        quote! {
            pub unsafe fn spawn_with_flags #generic_params(&self, flags: ::multiprocessing::subprocess::Flags, #(#fn_args,)*) -> ::std::io::Result<::multiprocessing::Child<#return_type>> {
                use ::multiprocessing::BindValue;
                ::multiprocessing::spawn(::std::boxed::Box::new(::multiprocessing::CallWrapper(#entry_ident:: #generics ::new(::std::boxed::Box::new(#bound)))), flags)
            }

            #[cfg(feature = "tokio")]
            pub async unsafe fn spawn_with_flags_tokio #generic_params(&self, flags: ::multiprocessing::subprocess::Flags, #(#fn_args,)*) -> ::std::io::Result<::multiprocessing::tokio::Child<#return_type>> {
                use ::multiprocessing::BindValue;
                ::multiprocessing::tokio::spawn(::std::boxed::Box::new(::multiprocessing::CallWrapper(#entry_ident:: #generics ::new(::std::boxed::Box::new(#bound)))), flags).await
            }

            pub fn spawn #generic_params(&self, #(#fn_args,)*) -> ::std::io::Result<::multiprocessing::Child<#return_type>> {
                unsafe { self.spawn_with_flags(::std::default::Default::default(), #(#arg_names,)*) }
            }

            #[cfg(feature = "tokio")]
            pub async fn spawn_tokio #generic_params(&self, #(#fn_args,)*) -> ::std::io::Result<::multiprocessing::tokio::Child<#return_type>> {
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
        #[derive(::multiprocessing::Object)]
        struct #entry_ident #generic_params {
            func: ::multiprocessing::Delayed<::std::boxed::Box<dyn ::multiprocessing::FnOnceObject<(), Output = #return_type_wrapped>>>,
            #(#generic_phantom,)*
        }

        impl #generic_params #entry_ident #generics {
            fn new(func: ::std::boxed::Box<dyn ::multiprocessing::FnOnceObject<(), Output = #return_type_wrapped>>) -> Self {
                Self {
                    func: ::multiprocessing::Delayed::new(func),
                    #(#generic_phantom_build,)*
                }
            }
        }

        impl #generic_params ::multiprocessing::InternalFnOnce<(::multiprocessing::handles::RawHandle,)> for #entry_ident #generics {
            type Output = i32;
            #tokio_attr
            #[allow(unreachable_code)] // If func returns !
            #async_ fn call_once(self, args: (::multiprocessing::handles::RawHandle,)) -> Self::Output {
                let output_tx_handle = args.0;
                use ::multiprocessing::handles::FromRawHandle;
                let mut output_tx = unsafe {
                    ::multiprocessing #ns_tokio ::Sender::<#return_type>::from_raw_handle(output_tx_handle)
                };
                output_tx.send(&self.func.deserialize()() #dot_await)
                    #dot_await
                    .expect("Failed to send subprocess output");
                0
            }
        }

        impl #generic_params ::multiprocessing::InternalFnOnce<(#(#fn_types,)*)> for #type_ident {
            type Output = #return_type_wrapped;
            fn call_once(self, args: (#(#fn_types,)*)) -> Self::Output {
                #pin(#type_ident::call(#(#args_from_tuple,)*))
            }
        }
        impl #generic_params ::multiprocessing::InternalFnMut<(#(#fn_types,)*)> for #type_ident {
            fn call_mut(&mut self, args: (#(#fn_types,)*)) -> Self::Output {
                #pin(#type_ident::call(#(#args_from_tuple,)*))
            }
        }
        impl #generic_params ::multiprocessing::InternalFn<(#(#fn_types,)*)> for #type_ident {
            fn call(&self, args: (#(#fn_types,)*)) -> Self::Output {
                #pin(#type_ident::call(#(#args_from_tuple,)*))
            }
        }

        #[allow(non_camel_case_types)]
        #[derive(::multiprocessing::Object)]
        #vis struct #type_ident;

        impl #type_ident {
            #[link_name = #link_name]
            #input

            #impl_code
        }

        #[allow(non_upper_case_globals)]
        #vis const #ident: ::multiprocessing::CallWrapper<#type_ident> = ::multiprocessing::CallWrapper(#type_ident);
    };

    TokenStream::from(expanded)
}

/// Setup an entrypoint.
///
/// This attribute must always be added to `fn main`:
///
/// ```rust
/// #[multiprocessing::main]
/// fn main() {
///     // ...
/// }
/// ```
///
/// Without it, starting child processes will not work. This might mean the application crashes,
/// starts infinitely many child processes or does something else you don't want.
///
/// This attribute may be mixed with other attributes, e.g. `#[tokio::main]`. In this case, this
/// attribute should be the first in the list:
///
/// ```rust
/// #[multiprocessing::main]
/// #[tokio::main]
/// async fn main() {
///     // ...
/// }
/// ```
#[proc_macro_attribute]
pub fn main(_meta: TokenStream, input: TokenStream) -> TokenStream {
    let mut input = parse_macro_input!(input as syn::ItemFn);

    input.sig.ident = syn::Ident::new("multiprocessing_old_main", input.sig.ident.span());

    let expanded = quote! {
        #input

        #[::multiprocessing::imp::ctor]
        fn multiprocessing_add_main() {
            *::multiprocessing::imp::MAIN_ENTRY
                .write()
                .expect("Failed to acquire write access to MAIN_ENTRY") = Some(|| {
                ::multiprocessing::imp::Report::report(multiprocessing_old_main())
            });
        }

        fn main() {
            ::multiprocessing::imp::main()
        }
    };

    TokenStream::from(expanded)
}

/// Make a structure or a enum serializable.
///
/// This derive macro enables the corresponding type to be passed via channels and to and from child
/// processes. [`Object`] can be implemented for a struct/enum if all of its fields implement
/// [`Object`]:
///
/// This is okay:
///
/// ```rust
/// # use multiprocessing::Object;
/// #[derive(Object)]
/// struct Test(String, i32);
/// ```
///
/// This is not okay:
///
/// ```compile_fail
/// # use multiprocessing::Object;
/// struct NotObject;
///
/// #[derive(Object)]
/// struct Test(String, i32, NotObject);
/// ```
///
/// Generics are supported. In this case, to ensure that all fields implement [`Object`],
/// constraints might be necessary:
///
/// This is okay:
///
/// ```rust
/// # use multiprocessing::Object;
/// #[derive(Object)]
/// struct MyPair<T: Object>(T, T);
/// ```
///
/// This is not okay:
///
/// ```compile_fail
/// # use multiprocessing::Object;
/// #[derive(Object)]
/// struct MyPair<T>(T, T);
/// ```
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
                            #ident: d.deserialize(),
                        }
                    });
                    quote! { Self { #(#deserialize_fields)* } }
                }
                syn::Fields::Unnamed(ref fields) => {
                    let deserialize_fields = fields.unnamed.iter().map(|_| {
                        quote! {
                            d.deserialize(),
                        }
                    });
                    quote! { Self (#(#deserialize_fields)*) }
                }
                syn::Fields::Unit => {
                    quote! { Self }
                }
            };

            quote! {
                impl #generics_impl ::multiprocessing::Object for #ident #generics #generics_where {
                    fn serialize_self(&self, s: &mut ::multiprocessing::Serializer) {
                        #(#serialize_fields)*
                    }
                    fn deserialize_self(d: &mut ::multiprocessing::Deserializer) -> Self {
                        #deserialize_fields
                    }
                    fn deserialize_on_heap<'serde>(&self, d: &mut ::multiprocessing::Deserializer) -> ::std::boxed::Box<dyn ::multiprocessing::Object + 'serde> where Self: 'serde {
                        use ::multiprocessing::Object;
                        ::std::boxed::Box::new(Self::deserialize_self(d))
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
                                quote! { #ident: d.deserialize() }
                            })
                            .collect();
                        quote! { #i => Self::#ident{ #(#des,)* } }
                    }
                    syn::Fields::Unnamed(fields) => {
                        let des: Vec<_> = (0..fields.unnamed.len())
                            .map(|_| quote! { d.deserialize() })
                            .collect();
                        quote! { #i => Self::#ident(#(#des,)*) }
                    }
                    syn::Fields::Unit => {
                        quote! { #i => Self::#ident }
                    }
                }
            });
            quote! {
                impl #generics_impl ::multiprocessing::Object for #ident #generics #generics_where {
                    fn serialize_self(&self, s: &mut ::multiprocessing::Serializer) {
                        match self {
                            #(#serialize_variants,)*
                        }
                    }
                    fn deserialize_self(d: &mut ::multiprocessing::Deserializer) -> Self {
                        match d.deserialize::<usize>() {
                            #(#deserialize_variants,)*
                            _ => panic!("Unexpected enum variant"),
                        }
                    }
                    fn deserialize_on_heap<'serde>(&self, d: &mut ::multiprocessing::Deserializer) -> ::std::boxed::Box<dyn ::multiprocessing::Object + 'serde> where Self: 'serde {
                        use ::multiprocessing::Object;
                        ::std::boxed::Box::new(Self::deserialize_self(d))
                    }
                }
            }
        }
        syn::Data::Union(_) => unimplemented!(),
    };

    TokenStream::from(expanded)
}
