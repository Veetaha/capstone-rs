use convert_case::Casing;
use proc_macro2::TokenStream as TokenStream2;
use quote::{format_ident, ToTokens};
use syn::{punctuated::Punctuated, Block, Signature};

pub fn process_capnproto_rpc(namespace: TokenStream2, item: syn::ItemImpl) -> TokenStream2 {
    let syn::ItemImpl { items, trait_, .. } = item;

    let generics = extract_generics_from_trait(trait_.clone());

    let mut new_items = Vec::new();
    for impl_item in items {
        let syn::ImplItem::Fn(impl_item_fn) = impl_item else {
            continue;
        };
        let to_append = process_fn_item(namespace.clone(), impl_item_fn, generics.clone());
        new_items.push(syn::ImplItem::Fn(to_append));
    }
    syn::ItemImpl {
        items: new_items,
        trait_,
        ..item
    }
    .into_token_stream()
}

fn extract_generics_from_trait(
    trait_: Option<(Option<syn::token::Not>, syn::Path, syn::token::For)>,
) -> syn::PathArguments {
    let path: syn::Path = trait_.unwrap().1;
    let path_arguments: &syn::PathArguments = &path.segments.last().unwrap().arguments;
    if let syn::PathArguments::AngleBracketed(_) = path_arguments {
        path_arguments.clone()
    } else {
        syn::PathArguments::None
    }
}

fn process_fn_item(
    namespace: TokenStream2,
    item: syn::ImplItemFn,
    generics: syn::PathArguments,
) -> syn::ImplItemFn {
    let syn::ImplItemFn { sig, block, .. } = item;
    let args = sig.inputs.iter().skip(1).map(|x| x.to_owned()).collect();
    let sig = process_signature(namespace, sig, generics);
    let block = process_block(block, args);

    syn::ImplItemFn { sig, block, ..item }
}

fn process_signature(
    namespace: TokenStream2,
    sig: Signature,
    generics: syn::PathArguments,
) -> Signature {
    // dbg!(generics.clone().into_token_stream().to_string());

    // TODO Instead of having it mutable - construct in place
    let mut inputs = Punctuated::<syn::FnArg, syn::token::Comma>::new();
    let type_prefix = sig.ident.to_string().to_case(convert_case::Case::Pascal);
    let params_type = format_ident!("{}Params", type_prefix);
    let params: syn::FnArg = syn::parse_quote!(params: #namespace::#params_type #generics);

    // TODO We're ignoring user's return type, might lead to confusion
    let result_type = format_ident!("{}Results", type_prefix);
    let result: syn::FnArg = syn::parse_quote!(mut results: #namespace::#result_type #generics); // just straight up output type
    inputs.push(sig.receiver().unwrap().to_owned().into());
    inputs.push(params);
    inputs.push(result);

    let output: syn::ReturnType = syn::parse_quote!(-> Result<(), capnp::Error>);

    Signature {
        inputs,
        output,
        ..sig
    }
}

fn process_block(block: Block, args: Vec<syn::FnArg>) -> Block {
    let Block { mut stmts, .. } = block;
    let rparams_stmt: syn::Stmt = syn::parse_quote! {
        let rparams = params.get()?;
    };

    let idents = args.into_iter().map(|x| match x {
        syn::FnArg::Receiver(_) => unreachable!(),
        // TODO We're ignoring type field ty
        syn::FnArg::Typed(syn::PatType { pat, .. }) => pat,
    });
    let capnp_let_stmt: syn::Stmt =
        syn::parse_quote! (::capnp_macros::capnp_let!({#(#idents),*} = rparams););
    stmts.insert(0, rparams_stmt);
    stmts.insert(1, capnp_let_stmt);
    Block { stmts, ..block }
}
