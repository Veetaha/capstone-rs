//! Download and/or build official Cap-n-Proto compiler (capnp) release for the current OS and architecture

use convert_case::{Case, Casing};
use eyre::{eyre, Result};
use proc_macro::TokenStream;
use proc_macro2::TokenStream as TokenStream2;
use quote::{format_ident, quote};
use std::path::PathBuf;
use std::str::FromStr;
use std::{env, fs, path::Path};
use syn::parse::Parser;
use syn::{LitStr, Token};
use walkdir::WalkDir;
use wax::{BuildError, Walk};

use eyre::Context;

include!(concat!(env!("OUT_DIR"), "/extract_bin.rs"));

/// `capnp_import!(pattern_1, pattern_2, ..., pattern_n)` compiles all the .capnp files at the locations of those files
/// and replaces itself with the resulting contents wrapped in appropriate module structure.
/// Resulting rust files from that compilation are then deleted.
#[proc_macro]
pub fn capnp_import(input: TokenStream) -> TokenStream {
    let parser = syn::punctuated::Punctuated::<LitStr, Token![,]>::parse_separated_nonempty;
    let path_patterns = parser.parse(input).unwrap();
    let path_patterns = path_patterns.into_iter().map(|item| item.value());
    let result = process_inner(path_patterns).unwrap();
    result.into()
}

#[proc_macro]
pub fn capnp_extract_bin(_: TokenStream) -> TokenStream {
    let content = std::fs::read_to_string(concat!(env!("OUT_DIR"), "/extract_bin.rs")).unwrap();
    TokenStream2::from_str(&content).unwrap().into()
}

fn process_inner<I>(path_patterns: I) -> Result<TokenStream2>
where
    I: IntoIterator,
    I::Item: AsRef<str>,
{
    let mut cmd = capnpc::CompilerCommand::new();

    let output_dir = commandhandle().context("could not create temporary capnp binary")?;
    let cmdpath = output_dir.path().join("capnp");
    cmd.capnp_executable(cmdpath);
    cmd.output_path(&output_dir);

    let searchpaths: Vec<PathBuf> = std::env::vars()
        .filter_map(|(key, value)| {
            if key.starts_with("DEP_") && key.ends_with("_SCHEMA_DIR") {
                cmd.import_path(&value);
                Some(value.into())
            } else {
                None
            }
        })
        .collect();

    let manifest: [PathBuf; 1] =
        [PathBuf::from_str(&std::env::var("CARGO_MANIFEST_DIR").unwrap()).unwrap()];

    let globs = path_patterns
        .into_iter()
        .flat_map(|s| {
            let is_absolute = s.as_ref().starts_with('/');
            let closure = move |dir| {
                wax::walk(s.as_ref().strip_prefix('/').unwrap_or(s.as_ref()), dir)
                    .map_err(BuildError::into_owned)
                    .map(Walk::into_owned)
            };
            if is_absolute {
                searchpaths.iter().flat_map(closure)
            } else {
                manifest.iter().flat_map(closure)
            }
        })
        .flatten();

    for entry_result in globs {
        let entry: PathBuf = entry_result?.into_path();
        if entry.is_file() {
            cmd.file(entry);
        }
    }

    cmd.run()?;
    let mut helperfile = TokenStream2::new();
    for entry_result in WalkDir::new(output_dir.path()) {
        let file_path = entry_result?.into_path();
        if file_path.is_file()
            && file_path
                .file_name()
                .ok_or(eyre!("Couldn't parse file: {:?}", file_path))?
                .to_str()
                .ok_or(eyre!("Couldn't convert to &str: {:?}", file_path))?
                .ends_with("_capnp.rs")
        {
            helperfile.extend(append_path(&file_path)?);
        }
    }
    Ok(helperfile)
    // When TempDir goes out of scope, it gets deleted
}

fn append_path(file_path: &Path) -> Result<TokenStream2> {
    let file_stem = file_path
        .file_stem()
        .ok_or(eyre!("Couldn't parse file: {:?}", file_path))?
        .to_str()
        .ok_or(eyre!("Couldn't convert to &str: {:?}", file_path))?
        .to_case(Case::Snake);

    let contents = TokenStream2::from_str(&fs::read_to_string(file_path)?).map_err(|_| {
        eyre!(
            "Couldn't convert file contents to TokenStream: {:?}",
            file_path
        )
    })?;
    let module_name = format_ident!("{}", file_stem);
    let helperfile = quote! {
        pub mod #module_name {
            #contents
        }
    };
    Ok(helperfile)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn basic_file_test() -> Result<()> {
        let contents = process_inner(["tests/example.capnp"])?.to_string();
        assert!(contents.starts_with("pub mod example_capnp {"));
        assert!(contents.ends_with('}'));
        Ok(())
    }

    #[test]
    fn glob_test() -> Result<()> {
        let contents = process_inner(["tests/folder-test/*.capnp"])?;
        let tests_module: syn::ItemMod = syn::parse2(contents)?;
        assert_eq!(tests_module.ident, "foo_capnp");
        Ok(())
    }

    #[test]
    fn search_test() -> Result<()> {
        let folder = PathBuf::from_str(&std::env::var("CARGO_MANIFEST_DIR")?)?.join("tests");
        std::env::set_var("DEP_TEST_SCHEMA_DIR", folder.as_os_str());
        let contents = process_inner(["/folder-test/*.capnp"])?;
        let tests_module: syn::ItemMod = syn::parse2(contents)?;
        assert_eq!(tests_module.ident, "foo_capnp");
        Ok(())
    }

    #[should_panic]
    #[test]
    fn search_failure_test() {
        let folder = PathBuf::from_str(&std::env::var("CARGO_MANIFEST_DIR").unwrap())
            .unwrap()
            .join("tests");
        std::env::set_var("DEP_TEST_SCHEMA_DIR", folder.as_os_str());
        // This should fail because the path doesn't start with '/'
        let contents = process_inner(["folder-test/*.capnp"]).unwrap();
        let tests_module: syn::ItemMod = syn::parse2(contents).unwrap();
        assert_eq!(tests_module.ident, "foo_capnp");
    }
}
