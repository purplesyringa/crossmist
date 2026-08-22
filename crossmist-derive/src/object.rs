use proc_macro::TokenStream;
use quote::{ToTokens, format_ident, quote};
use syn::{Data, DeriveInput, Fields, GenericParam, Index, parse_macro_input};

pub fn derive_object(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);

    let ident = &input.ident;

    let generics = {
        let params: Vec<_> = input
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

    let generic_params = &input.generics.params;
    let generics_impl = quote! { <#generic_params> };

    let generics_where = input.generics.where_clause;

    let expanded = match input.data {
        Data::Struct(struct_) => {
            let serialize_fields = match struct_.fields {
                Fields::Named(ref fields) => fields
                    .named
                    .iter()
                    .map(|field| {
                        let ident = &field.ident;
                        quote! {
                            s.serialize(self.#ident);
                        }
                    })
                    .collect(),
                Fields::Unnamed(ref fields) => fields
                    .unnamed
                    .iter()
                    .enumerate()
                    .map(|(i, _)| {
                        let i = Index::from(i);
                        quote! {
                            s.serialize(self.#i);
                        }
                    })
                    .collect(),
                Fields::Unit => Vec::new(),
            };

            let deserialize_fields = match struct_.fields {
                Fields::Named(ref fields) => {
                    let deserialize_fields = fields.named.iter().map(|field| {
                        let ident = &field.ident;
                        quote! {
                            #ident: unsafe { d.deserialize() },
                        }
                    });
                    quote! { Self { #(#deserialize_fields)* } }
                }
                Fields::Unnamed(ref fields) => {
                    let deserialize_fields = fields.unnamed.iter().map(|_| {
                        quote! {
                            unsafe { d.deserialize() },
                        }
                    });
                    quote! { Self (#(#deserialize_fields)*) }
                }
                Fields::Unit => {
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
        Data::Enum(enum_) => {
            let serialize_variants = enum_.variants.iter().enumerate().map(|(i, variant)| {
                let ident = &variant.ident;
                match &variant.fields {
                    Fields::Named(fields) => {
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
                    Fields::Unnamed(fields) => {
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
                    Fields::Unit => {
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
                    Fields::Named(fields) => {
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
                    Fields::Unnamed(fields) => {
                        let des: Vec<_> = (0..fields.unnamed.len())
                            .map(|_| quote! { unsafe { d.deserialize() } })
                            .collect();
                        quote! { #i => Ok(Self::#ident(#(#des,)*)) }
                    }
                    Fields::Unit => {
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
        Data::Union(_) => unimplemented!(),
    };

    TokenStream::from(expanded)
}
