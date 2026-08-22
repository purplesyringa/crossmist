use proc_macro::TokenStream;
use quote::{format_ident, quote};
use syn::{Data, DeriveInput, parse_macro_input};

pub fn derive_object(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);

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
        Data::Union(_) => unimplemented!(),
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

    let ident = &input.ident;
    let (impl_generics, type_generics, where_clause) = input.generics.split_for_impl();
    quote! {
        unsafe impl #impl_generics ::crossmist::Object for #ident #type_generics #where_clause {
            fn serialize_self(self, s: &mut ::crossmist::Serializer) {
                match self {
                    #(#serialize_variants)*
                }
            }
            unsafe fn deserialize_self(d: &mut ::crossmist::Deserializer) -> Self {
                match d.deserialize() {
                    #(#deserialize_variants)*
                    _ => panic!("Unexpected enum variant"),
                }
            }
        }
    }
    .into()
}
