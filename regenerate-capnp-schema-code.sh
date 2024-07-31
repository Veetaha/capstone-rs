#! /bin/sh

set -e
set -x

cargo build -p capstone-gen
capnp compile -otarget/debug/capnpc-rust-bootstrap:capnp/src capnp/schema.capnp --src-prefix capnp/
rustfmt capnp/src/schema_capnp.rs
