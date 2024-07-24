//! Download and/or build official Cap-n-Proto compiler (capnp) release for the current OS and architecture

use convert_case::{Case, Casing};
use eyre::{eyre, Result};
use proc_macro::TokenStream;
use proc_macro2::TokenStream as TokenStream2;
use quote::{format_ident, quote};
use std::hash::{DefaultHasher, Hash, Hasher};
use std::path::PathBuf;
use std::str::FromStr;
use std::{env, fs, path::Path};
use syn::parse::Parser;
use syn::{LitStr, Token};
use walkdir::WalkDir;
use wax::Glob;

use eyre::Context;

include!(concat!(env!("OUT_DIR"), "/extract_bin.rs"));

/// `capnp_import!(pattern_1, pattern_2, ..., pattern_n)` compiles all the .capnp files at the locations of those files
/// and replaces itself with the resulting contents wrapped in appropriate module structure.
/// Resulting rust files from that compilation are then deleted.
#[proc_macro]
pub fn capnp_import(input: TokenStream) -> TokenStream {
    let parser = syn::punctuated::Punctuated::<LitStr, Token![,]>::parse_separated_nonempty;
    let path_patterns = parser.parse(input).unwrap();
    let path_patterns: Vec<String> = path_patterns.into_iter().map(|item| item.value()).collect();

    let mut hasher = DefaultHasher::new();
    path_patterns.hash(&mut hasher);

    let helperfile = process_inner(&path_patterns).unwrap();
    if let Ok(out_dir) = std::env::var("OUT_DIR") {
        let file_path = PathBuf::from_str(&format!("{}/{}.rs", out_dir, hasher.finish())).unwrap();
        let _ = fs::write(&file_path, helperfile.to_string());
        let file_out = file_path.to_string_lossy().to_string();

        quote! {
            include!(#file_out);
        }
        .into()
    } else {
        helperfile.into()
    }
}

#[proc_macro]
pub fn capnp_extract_bin(_: TokenStream) -> TokenStream {
    let content = std::fs::read_to_string(concat!(env!("OUT_DIR"), "/extract_bin.rs")).unwrap();
    TokenStream2::from_str(&content).unwrap().into()
}

fn process_inner(path_patterns: &[impl AsRef<str>]) -> Result<TokenStream2> {
    if path_patterns.is_empty() {
        return Err(eyre!("No search patterns for capnp files specified"));
    }

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

    let manifest: [PathBuf; 1] = [PathBuf::from_str(&std::env::var("CARGO_MANIFEST_DIR")?)?];

    let glob_matches = path_patterns
        .iter()
        .map(|pattern| -> eyre::Result<_> {
            let pattern = pattern.as_ref();
            let (search_prefix, glob) = Glob::new(pattern.trim_start_matches('/'))?.partition();
            Ok((pattern, search_prefix, glob))
        }).map(|maybe_pattern| {
            match maybe_pattern {
                Ok((pattern, search_prefix, glob)) => {
                    let initial_paths = if pattern.starts_with('/')  { &*searchpaths } else { &manifest };
                    let mut ensure_some = initial_paths
                    .iter()
                    .flat_map(move |dir: &PathBuf| -> _ {
                        // build glob and partition it into a static prefix and shorter glob pattern
                        // For example, converts "../schemas/*.capnp" into Path(../schemas) and Glob(*.capnp)
                        glob.walk(dir.join(&search_prefix)).into_owned().flatten()
                    }).peekable();
                    if ensure_some.peek().is_none() {
                        return Err(eyre!(
                            "No capnp files found matching {pattern}, did you mean to use an absolute path instead of a relative one?
                  Manifest directory for relative paths: {:#?}
                  Potential directories for absolute paths: {:#?}",
                            manifest,
                            searchpaths
                        ));
                    }
                    Ok(ensure_some)
                },
                Err(err) => Err(err),
            }
        });

    for entry in glob_matches {
        for entry in entry? {
            if entry.file_type().is_file() {
                cmd.file(entry.path());
            }
        }
    }

    if cmd.file_count() == 0 {
        // I think the only way we can reach this now is by failing the is_file() check above
        return Err(eyre!(
            "No capnp files found, did you mean to use an absolute path instead of a relative one?
  Manifest directory for relative paths: {:#?}
  Potential directories for absolute paths: {:#?}",
            manifest,
            searchpaths
        ));
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
    use serial_test::serial;

    #[test]
    fn basic_file_test() -> Result<()> {
        let contents = process_inner(&vec!["tests/example.capnp".to_string()])?.to_string();
        assert!(contents.starts_with("pub mod example_capnp {"));
        assert!(contents.ends_with('}'));
        Ok(())
    }

    #[test]
    fn relative_file_test() -> Result<()> {
        // NOTE: if the project name changes this test will have to be changed
        let contents =
            process_inner(&vec!["../capnp-import/tests/example.capnp".to_string()])?.to_string();
        assert!(contents.starts_with("pub mod example_capnp {"));
        assert!(contents.ends_with('}'));
        Ok(())
    }

    #[test]
    fn glob_test() -> Result<()> {
        let contents = process_inner(&vec!["tests/folder-test/*.capnp".to_string()])?;
        let tests_module: syn::ItemMod = syn::parse2(contents)?;
        assert_eq!(tests_module.ident, "foo_capnp");
        Ok(())
    }

    #[should_panic]
    #[test]
    fn compile_fail_test() {
        let _ = process_inner(&vec!["*********//*.capnp*".to_string()]).unwrap();
    }

    /// Confirm wax partition is handling ../ as expected since its handling of parent dirs has varied across releases
    #[test]
    fn wax_partition_handles_parent_dir() {
        let (search_prefix, _glob) = Glob::new("../schemas/**/*.capnp").unwrap().partition();
        assert_eq!(search_prefix, PathBuf::new().join("..").join("schemas"),);
    }

    fn project_subdir(dir: &str) -> Result<PathBuf> {
        Ok(PathBuf::from_str(&std::env::var("CARGO_MANIFEST_DIR")?)?.join(dir))
    }

    #[test]
    #[serial]
    fn search_test() -> Result<()> {
        let folder = project_subdir("tests")?;
        std::env::set_var("DEP_TEST_SCHEMA_DIR", folder.as_os_str());
        let contents = process_inner(&vec!["/folder-test/*.capnp".to_string()])?;
        std::env::remove_var("DEP_TEST_SCHEMA_DIR");
        let tests_module: syn::ItemMod = syn::parse2(contents)?;
        assert_eq!(tests_module.ident, "foo_capnp");
        Ok(())
    }

    #[should_panic]
    #[serial]
    #[test]
    fn search_fail2_test() {
        let folder = project_subdir("tests").unwrap();
        std::env::set_var("DEP_TEST_WRONG_DIR", folder.as_os_str());
        // Should fail because DEP_TEST_WRONG_DIR is in the wrong format
        let contents = process_inner(&vec!["/folder-test/*.capnp".to_string()]);
        std::env::remove_var("DEP_TEST_WRONG_DIR");
        contents.unwrap();
    }

    #[should_panic]
    #[serial]
    #[test]
    fn search_failure_test() {
        let folder = project_subdir("tests").unwrap();
        std::env::set_var("DEP_TEST_SCHEMA_DIR", folder.as_os_str());
        // This should fail because the path doesn't start with '/'
        let contents = process_inner(&vec!["folder-test/*.capnp".to_string()]);
        std::env::remove_var("DEP_TEST_SCHEMA_DIR");
        contents.unwrap();
    }

    #[should_panic]
    #[serial]
    #[test]
    fn partial_failure_test() {
        let folder = project_subdir("tests").unwrap();
        std::env::set_var("DEP_TEST_SCHEMA_DIR", folder.as_os_str());

        // This should fail because the second path doesn't start with '/'
        let contents = process_inner(&vec![
            "tests/example.capnp".to_string(),
            "folder-test/*.capnp".to_string(),
        ]);
        std::env::remove_var("DEP_TEST_SCHEMA_DIR");
        contents.unwrap();
    }

    #[serial]
    #[test]
    fn eventual_success_test_empty() {
        let folder = project_subdir("tests").unwrap();
        let empty_folder = project_subdir("tests_but_emptier").unwrap();
        fs::create_dir_all(&empty_folder).unwrap();
        std::env::set_var("DEP_TEST_SCHEMA_DIR", folder.as_os_str());
        std::env::set_var("DEP_IGNORE_SCHEMA_DIR", empty_folder.as_os_str());

        // This should succeed despite DEP_IGNORE_SCHEMA_DIR being empty
        let contents = process_inner(&vec![
            "tests/example.capnp".to_string(),
            "/folder-test/*.capnp".to_string(),
        ]);
        std::env::remove_var("DEP_TEST_SCHEMA_DIR");
        std::env::remove_var("DEP_IGNORE_SCHEMA_DIR");
        contents.unwrap();
    }

    #[serial]
    #[test]
    fn eventual_success_test_no_matches() {
        let folder = project_subdir("tests").unwrap();
        let src_folder = project_subdir("src").unwrap();
        std::env::set_var("DEP_TEST_SCHEMA_DIR", folder.as_os_str());
        std::env::set_var("DEP_EMPTY_SCHEMA_DIR", src_folder.as_os_str());

        // This should succeed despite DEP_IGNORE_SCHEMA_DIR being a folder with no matches
        let contents = process_inner(&vec![
            "tests/example.capnp".to_string(),
            "/folder-test/*.capnp".to_string(),
        ]);
        std::env::remove_var("DEP_TEST_SCHEMA_DIR");
        std::env::remove_var("DEP_IGNORE_SCHEMA_DIR");
        contents.unwrap();
    }
}
