use proc_macro::TokenStream;
use quote::quote;
use syn::{
    ExprClosure, Ident, Pat, Result, Token, parenthesized,
    parse::{Parse, ParseStream},
    parse_macro_input, parse_quote,
    punctuated::Punctuated,
    token::Paren,
};

struct ExplicitlyCapturingClosure {
    captures: Option<ExplicitCaptures>,
    closure: ExprClosure,
}

#[allow(dead_code)]
struct ExplicitCaptures {
    move_token: Token![move],
    paren_token: Paren,
    captures: Punctuated<ExplicitCapture, Token![,]>,
}

struct ExplicitCapture {
    by_ref: Option<Token![ref]>,
    mutability: Option<Token![mut]>,
    ident: Ident,
}

impl Parse for ExplicitlyCapturingClosure {
    fn parse(input: ParseStream) -> Result<Self> {
        let mut captures = None;
        if input.peek(Token![move]) {
            captures = Some(input.parse()?);
        }
        let closure = input.parse()?;
        Ok(Self { captures, closure })
    }
}

impl Parse for ExplicitCaptures {
    fn parse(input: ParseStream) -> Result<Self> {
        let captures;
        Ok(Self {
            move_token: input.parse()?,
            paren_token: parenthesized!(captures in input),
            captures: captures.parse_terminated(ExplicitCapture::parse)?,
        })
    }
}

impl Parse for ExplicitCapture {
    fn parse(input: ParseStream) -> Result<Self> {
        let by_ref: Option<Token![ref]> = input.parse()?;
        let mutability = if by_ref.is_some() {
            input.parse()?
        } else {
            None
        };
        Ok(Self {
            by_ref,
            mutability,
            ident: input.parse()?,
        })
    }
}

pub fn func(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as ExplicitlyCapturingClosure);

    let mut captured_by_value = Vec::new();
    let mut captured_by_ref = Vec::new();
    let mut captured_by_ref_mut = Vec::new();
    if let Some(ExplicitCaptures { captures, .. }) = input.captures {
        for capture in captures {
            let group = if capture.by_ref.is_none() {
                &mut captured_by_value
            } else if capture.mutability.is_none() {
                &mut captured_by_ref
            } else {
                &mut captured_by_ref_mut
            };
            group.push(capture.ident);
        }
    }

    // Add captures and group arguments into a single tuple so that `crossmist` has an easier time
    // defining `Closure`.
    let mut closure = input.closure;
    let (input_patterns, input_types): (Vec<_>, Vec<_>) = closure
        .inputs
        .into_iter()
        .map(|input| match input {
            Pat::Type(pattype) => (*pattype.pat, pattype.ty),
            pat => (pat, parse_quote! { _ }),
        })
        .unzip();
    let new_inputs: ExprClosure = parse_quote! {
        |
            (#(#captured_by_value,)*): _,
            (#(#captured_by_ref,)*): &_,
            (#(#captured_by_ref_mut,)*): &mut _,
            (#(#input_patterns,)*): (#(#input_types,)*),
        | {}
    };
    closure.inputs = new_inputs.inputs;

    let input_underscores = closure
        .inputs
        .iter()
        .map(|_| quote! { _ })
        .collect::<Vec<_>>();

    quote! {
        {
            // The closure must be part of the same expression that links the bound identifiers to
            // their values, so that bound types can inferred.
            let closure = ::crossmist::Closure::unsafe_new(
                #closure,
                (#(#captured_by_value,)*),
                (#(#captured_by_ref,)*),
                (#(#captured_by_ref_mut,)*),
            );
            // assert that the closure doesn't borrow
            let _ = closure.conjure() as fn(#(#input_underscores,)*) -> _;
            ::std::boxed::Box::new(closure)
        }
    }
    .into()
}
