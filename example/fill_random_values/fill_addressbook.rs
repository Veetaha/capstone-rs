use capnp::{dynamic_struct, dynamic_value};
use fill_random_values::Filler;

capnp_import::capnp_import!("fill.capnp");
capnp_import::capnp_import!("addressbook.capnp");

pub fn main() {
    let mut message = ::capnp::message::Builder::new_default();
    let mut addressbook = message.init_root::<addressbook_capnp::address_book::Builder>();

    let mut filler = Filler::new(::rand::thread_rng(), 10);
    let dynamic: dynamic_value::Builder = addressbook.reborrow().into();
    filler.fill(dynamic.downcast()).unwrap();

    // Ensure we can downcast back to our original struct builder
    let dynamic: dynamic_value::Builder = addressbook.reborrow().into();
    let downcast: dynamic_struct::Builder = dynamic.downcast();
    let specific: addressbook_capnp::address_book::Builder = downcast.downcast().unwrap();
    println!("{:#?}", specific.into_reader());
}
