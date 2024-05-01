capnp_import::capnp_import!("tests/test_schema.capnp");

use capnp_macros::{capnp_build, capnproto_rpc};
use test_schema_capnp::generic_interface;

#[derive(Default)]
struct GenericInterfaceImpl<T> {
    value: T,
}

type T0 = capnp::text::Owned;

#[capnproto_rpc(generic_interface)]
impl generic_interface::Server<T0> for GenericInterfaceImpl<T0> {
    async fn generic_set_value(&self, value: GenericInterfaceImpl<T0>) -> Result<(), capnp::Error> {
        Ok(())
    }

    async fn generic_get_value(&self) {
        Ok(())
    }
}

// Mostly to show that it compiles, I'm not sure how to instantiate
#[tokio::test]
async fn decorator_generic_test() -> capnp::Result<()> {
    let _client: generic_interface::Client<T0>;
    Ok(())
}
