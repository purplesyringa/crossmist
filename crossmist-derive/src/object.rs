use darling::FromDeriveInput;
use proc_macro::TokenStream;
use quote::{format_ident, quote};
use syn::{Data, DeriveInput, WherePredicate, parse_macro_input, parse_quote};

#[derive(FromDeriveInput)]
#[darling(attributes(crossmist))]
pub struct ObjectOpts {
    bound: Option<Vec<WherePredicate>>,
}

pub fn derive_object(input: TokenStream) -> TokenStream {
    let mut input = parse_macro_input!(input as DeriveInput);
    let opts = match ObjectOpts::from_derive_input(&input) {
        Ok(opts) => opts,
        Err(err) => return err.write_errors().into(),
    };

    let variants = match input.data {
        Data::Struct(struct_) => vec![(struct_.fields, quote! { Self })],
        Data::Enum(enum_) => enum_
            .variants
            .into_iter()
            .map(|variant| {
                let variant_ident = variant.ident;
                (variant.fields, quote! { Self::#variant_ident })
            })
            .collect(),
        Data::Union(_) => {
            return quote! { compile_error!("this trait cannot be derived for unions"); }.into();
        }
    };

    let discriminant_type = if variants.len() <= 1 {
        // explicitly handle the 0-variant case because type inference fails otherwise
        quote! { () }
    } else {
        quote! { usize }
    };

    let (serialize_variants, deserialize_variants): (Vec<_>, Vec<_>) = variants
        .iter()
        .enumerate()
        .map(|(i, (fields, ctor))| {
            let discriminant = if variants.len() == 1 {
                quote! { () } // structs and single-variant enums
            } else {
                quote! { #i }
            };
            let members_ser = fields.members();
            let members_de = fields.members();
            let bindings: Vec<_> = (0..fields.len()).map(|i| format_ident!("a{}", i)).collect();
            (
                quote! {
                    #ctor { #(#members_ser: #bindings,)* } => {
                        s.serialize(#discriminant);
                        #(s.serialize(#bindings);)*
                    },
                },
                quote! {
                    #discriminant => #ctor {
                        #(#members_de: unsafe { d.deserialize() },)*
                    },
                },
            )
        })
        .unzip();

    let bound: Vec<WherePredicate> = opts.bound.unwrap_or_else(|| {
        input
            .generics
            .type_params()
            .map(|type_param| {
                let ident = &type_param.ident;
                parse_quote! { #ident: ::crossmist::Object }
            })
            .collect()
    });
    input.generics.make_where_clause().predicates.extend(bound);

    let ident = &input.ident;
    let (impl_generics, type_generics, where_clause) = input.generics.split_for_impl();
    quote! {
        unsafe impl #impl_generics ::crossmist::Object for #ident #type_generics #where_clause {
            fn serialize_self(self, s: &mut ::crossmist::serde::Serializer) {
                match self {
                    #(#serialize_variants)*
                }
            }
            unsafe fn deserialize_self(d: &mut ::crossmist::serde::Deserializer) -> Self {
                match d.deserialize::<#discriminant_type>() {
                    #(#deserialize_variants)*
                    _ => panic!("Unexpected enum variant"),
                }
            }
        }
    }
    .into()
}
