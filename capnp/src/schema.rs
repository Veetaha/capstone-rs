//! Convenience wrappers of the datatypes defined in schema.capnp.

use crate::dynamic_value;
use crate::introspect::{
    self, RawBrandedStructSchema, RawCapabilitySchema, RawEnumSchema, TypeVariant,
};
use crate::private::layout;
use crate::schema_capnp::{annotation, enumerant, field, node};
use crate::struct_list;
use crate::traits::{IndexMove, ListIter, ShortListIter};
use crate::Result;

#[cfg(feature = "std")]
use std::collections::hash_map::HashMap;

#[cfg(all(feature = "std", feature = "alloc"))]
// Builds introspection information at runtime to allow building a StructSchema
pub struct DynamicSchema {
    msg: crate::message::Reader<crate::serialize::OwnedSegments>,
    scopes: HashMap<(u64, String), u64>,
    node_parents: HashMap<u64, u64>,
    nodes: HashMap<u64, TypeVariant>,
    root: u64,
}

//const NAME_ANNOTATION_ID: u64 = 0xc2fe4c6d100166d0;
//const PARENT_MODULE_ANNOTATION_ID: u64 = 0xabee386cd1450364;
//const OPTION_ANNOTATION_ID: u64 = 0xabfef22c4ee1964e;

#[no_mangle]
#[inline(never)]
fn dynamic_field_marker(_: u16) -> crate::introspect::Type {
    panic!("dynamic_field_marker should never be called!");
}

#[no_mangle]
#[inline(never)]
fn dynamic_annotation_marker(_: Option<u16>, _: u32) -> crate::introspect::Type {
    panic!("dynamic_annotation_marker should never be called!");
}

#[cfg(all(feature = "std", feature = "alloc"))]
impl DynamicSchema {
    fn get_indexes(
        st: crate::schema_capnp::node::struct_::Reader,
    ) -> (&'static mut [u16], &'static mut [u16]) {
        let mut union_member_indexes = vec![];
        let mut nonunion_member_indexes = vec![];
        for (index, field) in st.get_fields().unwrap().iter().enumerate() {
            let disc = field.get_discriminant_value();
            if disc == crate::schema_capnp::field::NO_DISCRIMINANT {
                nonunion_member_indexes.push(index as u16);
            } else {
                union_member_indexes.push((disc, index as u16));
            }
        }
        union_member_indexes.sort();
        let members_by_discriminant: Vec<u16> =
            union_member_indexes.iter().map(|(_, d)| *d).collect();
        let nonunion_member_indexes: &'static mut [u16] =
            Box::leak(nonunion_member_indexes.into_boxed_slice());
        let members_by_discriminant: &'static mut [u16] =
            Box::leak(members_by_discriminant.into_boxed_slice());

        (nonunion_member_indexes, members_by_discriminant)
    }

    // Capnproto-rust doesn't believe in lifetimes so we get to do manual memory management! IN RUST!
    fn leak_chunk<T: crate::traits::SetPointerBuilder>(
        value: T,
        total_size: crate::MessageSize,
    ) -> Result<&'static mut [crate::Word]> {
        let allocator = crate::message::HeapAllocator::new()
            .first_segment_words(total_size.word_count as u32 + 1);
        let mut message = crate::message::Builder::new(allocator);
        message.set_root(value)?;
        let segment = message.get_segments_for_output()[0];
        if segment.len() % 8 != 0 {
            panic!("Segment invalid size!");
        }
        let boxed = unsafe {
            let p = std::alloc::alloc(std::alloc::Layout::from_size_align_unchecked(
                segment.len(),
                8,
            ));
            std::slice::from_raw_parts_mut(p, segment.len()).copy_from_slice(segment);
            Box::from_raw(std::slice::from_raw_parts_mut(
                p as *mut crate::Word,
                segment.len() / 8,
            ))
        };
        Ok(Box::leak(boxed))
    }

    fn process_node(
        nodes: &mut HashMap<u64, TypeVariant>,
        id: u64,
        scopes: &mut HashMap<(u64, String), u64>,
        node_map: &HashMap<u64, crate::schema_capnp::node::Reader>,
    ) -> Result<()> {
        let node = &node_map[&id];

        for nested in node.get_nested_nodes()? {
            scopes.insert((id, nested.get_name()?.to_string()?), nested.get_id());
        }

        match node.which()? {
            node::File(()) => {
                // do nothing
            }
            node::Struct(st) => {
                let (nonunion_member_indexes, members_by_discriminant) = Self::get_indexes(st);
                let leak = Self::leak_chunk(*node, node.total_size()?)?;
                let raw = Box::leak(Box::new(introspect::RawStructSchema {
                    encoded_node: leak,
                    nonunion_members: nonunion_member_indexes,
                    members_by_discriminant,
                }));

                let schema = crate::introspect::RawBrandedStructSchema {
                    generic: raw,
                    field_types: dynamic_field_marker,
                    annotation_types: dynamic_annotation_marker,
                    dynamic_schema: None, // TODO: this is impossible
                };

                nodes.insert(id, TypeVariant::Struct(schema));
            }
            node::Const(c) => {
                match c.get_type()?.which()? {
                    crate::schema_capnp::type_::Which::Void(_) => {
                        nodes.insert(id, TypeVariant::Void)
                    }
                    crate::schema_capnp::type_::Which::Bool(_) => {
                        nodes.insert(id, TypeVariant::Bool)
                    }
                    crate::schema_capnp::type_::Which::Int8(_) => {
                        nodes.insert(id, TypeVariant::Int8)
                    }
                    crate::schema_capnp::type_::Which::Int16(_) => {
                        nodes.insert(id, TypeVariant::Int16)
                    }
                    crate::schema_capnp::type_::Which::Int32(_) => {
                        nodes.insert(id, TypeVariant::Int32)
                    }
                    crate::schema_capnp::type_::Which::Int64(_) => {
                        nodes.insert(id, TypeVariant::Int64)
                    }
                    crate::schema_capnp::type_::Which::Uint8(_) => {
                        nodes.insert(id, TypeVariant::UInt8)
                    }
                    crate::schema_capnp::type_::Which::Uint16(_) => {
                        nodes.insert(id, TypeVariant::UInt16)
                    }
                    crate::schema_capnp::type_::Which::Uint32(_) => {
                        nodes.insert(id, TypeVariant::UInt32)
                    }
                    crate::schema_capnp::type_::Which::Uint64(_) => {
                        nodes.insert(id, TypeVariant::UInt64)
                    }
                    crate::schema_capnp::type_::Which::Float32(_) => {
                        nodes.insert(id, TypeVariant::Float32)
                    }
                    crate::schema_capnp::type_::Which::Float64(_) => {
                        nodes.insert(id, TypeVariant::Float64)
                    }
                    crate::schema_capnp::type_::Which::Text(_) => {
                        nodes.insert(id, TypeVariant::Text)
                    }
                    crate::schema_capnp::type_::Which::Data(_) => {
                        nodes.insert(id, TypeVariant::Data)
                    }
                    crate::schema_capnp::type_::Which::List(_) => todo!(),
                    crate::schema_capnp::type_::Which::Enum(_) => todo!(),
                    crate::schema_capnp::type_::Which::Struct(_) => todo!(),
                    crate::schema_capnp::type_::Which::Interface(_) => todo!(),
                    crate::schema_capnp::type_::Which::AnyPointer(_) => todo!(),
                };
            }
            node::Annotation(_) => {
                // Because annotations do not add a type to the nodes hashmap, and AnnotationList relies on reading the actual
                // encoded node for everything other than the type, we don't actually need to do anything with an annotation node.
            }
            node::Enum(_) => {
                let leak = Self::leak_chunk(*node, node.total_size()?)?;
                nodes.insert(
                    id,
                    TypeVariant::Enum(RawEnumSchema {
                        encoded_node: leak,
                        annotation_types: dynamic_annotation_marker,
                    }),
                );
            }
            node::Interface(_) => {
                let leak = Self::leak_chunk(*node, node.total_size()?)?;
                nodes.insert(
                    id,
                    TypeVariant::Capability(RawCapabilitySchema { encoded_node: leak }),
                );
            }
        }

        Ok(())
    }

    pub fn new(msg: crate::message::Reader<crate::serialize::OwnedSegments>) -> Result<Self> {
        let mut this = Self {
            msg,
            scopes: HashMap::new(),
            node_parents: HashMap::new(),
            nodes: HashMap::new(),
            root: 0,
        };

        let request: crate::schema_capnp::code_generator_request::Reader = this.msg.get_root()?;
        let mut node_map: HashMap<u64, crate::schema_capnp::node::Reader> = HashMap::new();
        let mut nodes = Box::leak(Box::new(HashMap::new()));

        for node in request.get_nodes()? {
            node_map.insert(node.get_id(), node);
            this.node_parents.insert(node.get_id(), node.get_scope_id());
        }

        // Fix up "anonymous" method params and results scopes.
        for node in request.get_nodes()? {
            if let Ok(crate::schema_capnp::node::Interface(interface_reader)) = node.which() {
                for method in interface_reader.get_methods()? {
                    let param_struct_type = method.get_param_struct_type();
                    if this.node_parents.get(&param_struct_type) == Some(&0) {
                        this.node_parents.insert(param_struct_type, node.get_id());
                    }
                    let result_struct_type = method.get_result_struct_type();
                    if this.node_parents.get(&result_struct_type) == Some(&0) {
                        this.node_parents.insert(result_struct_type, node.get_id());
                    }
                }
            }
        }

        // Fix up imported files
        for requested_file in request.get_requested_files()? {
            let id = requested_file.get_id();

            for import in requested_file.get_imports()? {
                let import_id = import.get_id();
                if this.node_parents.get(&import_id) == Some(&0) {
                    this.node_parents.insert(import_id, id);
                }
                this.scopes
                    .insert((id, import.get_name()?.to_string()?), import_id);
            }
        }

        for node in request.get_nodes()? {
            if this.node_parents[&node.get_id()] == 0 {
                if this.root != 0 {
                    return Err(crate::Error::from_kind(
                        crate::ErrorKind::MessageIsTooDeeplyNestedOrContainsCycles,
                    ));
                }
                this.root = node.get_id();
            }

            Self::process_node(nodes, node.get_id(), &mut this.scopes, &node_map)?;
        }
        Ok(this)
    }

    pub fn get_type_by_id(&self, id: u64) -> Option<&TypeVariant> {
        self.nodes.get(&id)
    }

    pub fn get_type_by_scope(&self, scope: Vec<String>) -> Option<&TypeVariant> {
        let mut parent = self.root;
        let mut result = None;

        for name in scope {
            let key = &(parent, name);
            result = self.scopes.get(key);
            if let Some(x) = result {
                parent = *x;
            } else {
                return None;
            }
        }

        if let Some(k) = result {
            self.nodes.get(k)
        } else {
            None
        }
    }
}

#[cfg(all(feature = "std", feature = "alloc"))]
impl std::ops::Drop for DynamicSchema {
    fn drop(&mut self) {
        // To clean up our mess of memory we have to iterate through all our types
        // and start re-capturing the raw pointers into boxes, like trying to herd
        // a bunch of extremely unsafe squirrels that got loose.

        for v in self.nodes.values_mut() {
            match v {
                TypeVariant::Struct(s) => {
                    let _ = unsafe {
                        Box::from_raw(
                            s.generic.encoded_node as *const [crate::Word] as *mut [crate::Word],
                        )
                    };
                    let _ = unsafe {
                        Box::from_raw(
                            s.generic.members_by_discriminant as *const [u16] as *mut [u16],
                        )
                    };
                    let _ = unsafe {
                        Box::from_raw(s.generic.nonunion_members as *const [u16] as *mut [u16])
                    };
                    let _ = unsafe {
                        Box::from_raw(
                            s.generic as *const crate::introspect::RawStructSchema
                                as *mut crate::introspect::RawStructSchema,
                        )
                    };
                }
                TypeVariant::Enum(e) => {
                    let _ = unsafe {
                        Box::from_raw(e.encoded_node as *const [crate::Word] as *mut [crate::Word])
                    };
                }
                TypeVariant::Capability(c) => {
                    let _ = unsafe {
                        Box::from_raw(c.encoded_node as *const [crate::Word] as *mut [crate::Word])
                    };
                }
                TypeVariant::List(_) => todo!(),
                _ => (), // do nothing unless it's something we allocated memory for
            }
        }
    }
}

/// A struct node, with generics applied.
#[derive(Clone, Copy)]
pub struct StructSchema {
    pub(crate) raw: RawBrandedStructSchema,
    pub(crate) proto: node::Reader<'static>,
}

impl StructSchema {
    pub fn new(raw: RawBrandedStructSchema) -> Self {
        let proto =
            crate::any_pointer::Reader::new(unsafe {
                layout::PointerReader::get_root_unchecked(
                    raw.generic.encoded_node.as_ptr() as *const u8
                )
            })
            .get_as()
            .unwrap();
        Self { raw, proto }
    }

    pub fn get_proto(&self) -> node::Reader<'static> {
        self.proto
    }

    pub fn get_fields(self) -> crate::Result<FieldList> {
        if let node::Struct(s) = self.proto.which()? {
            Ok(FieldList {
                fields: s.get_fields()?,
                parent: self,
            })
        } else {
            panic!()
        }
    }

    pub fn get_field_by_discriminant(self, discriminant: u16) -> Result<Option<Field>> {
        match self
            .raw
            .generic
            .members_by_discriminant
            .get(discriminant as usize)
        {
            None => Ok(None),
            Some(&idx) => Ok(Some(self.get_fields()?.get(idx))),
        }
    }

    /// Looks up a field by name. Returns `None` if no matching field is found.
    pub fn find_field_by_name(&self, name: &str) -> Result<Option<Field>> {
        for field in self.get_fields()? {
            if field.get_proto().get_name()? == name {
                return Ok(Some(field));
            }
        }
        Ok(None)
    }

    /// Like `find_field_by_name()`, but returns an error if the field is not found.
    pub fn get_field_by_name(&self, name: &str) -> Result<Field> {
        if let Some(field) = self.find_field_by_name(name)? {
            Ok(field)
        } else {
            let mut error = crate::Error::from_kind(crate::ErrorKind::FieldNotFound);
            write!(error, "{}", name);
            Err(error)
        }
    }

    pub fn get_union_fields(self) -> Result<FieldSubset> {
        if let node::Struct(s) = self.proto.which()? {
            Ok(FieldSubset {
                fields: s.get_fields()?,
                indices: self.raw.generic.members_by_discriminant,
                parent: self,
            })
        } else {
            panic!()
        }
    }

    pub fn get_non_union_fields(self) -> Result<FieldSubset> {
        if let node::Struct(s) = self.proto.which()? {
            Ok(FieldSubset {
                fields: s.get_fields()?,
                indices: self.raw.generic.nonunion_members,
                parent: self,
            })
        } else {
            panic!()
        }
    }

    pub fn get_annotations(self) -> Result<AnnotationList> {
        Ok(AnnotationList {
            annotations: self.proto.get_annotations()?,
            child_index: None,
            get_annotation_type: self.raw.annotation_types,
        })
    }
}

impl From<RawBrandedStructSchema> for StructSchema {
    fn from(rs: RawBrandedStructSchema) -> StructSchema {
        StructSchema::new(rs)
    }
}

/// A field of a struct, with generics applied.
#[derive(Clone, Copy)]
pub struct Field {
    proto: field::Reader<'static>,
    index: u16,
    pub(crate) parent: StructSchema,
}

impl Field {
    pub fn get_proto(self) -> field::Reader<'static> {
        self.proto
    }

    pub fn get_type(&self) -> introspect::Type {
        #[allow(clippy::fn_address_comparisons)]
        if self.parent.raw.field_types == dynamic_field_marker {
            let mut found: Option<crate::schema_capnp::type_::Reader> = None;
            for (index, field) in self.parent.get_fields().unwrap().iter().enumerate() {
                if index as u16 == self.index {
                    found = match field.get_proto().which().unwrap() {
                        field::Slot(slot) => slot.get_type().ok(),
                        field::Group(_) => {
                            // group.get_type_id() // need access to type mapping to find group's type node
                            panic!("don't know how to do groups yet");
                        }
                    };
                }
            }

            // If anything goes wrong we have to panic anyway
            match found.unwrap().which().unwrap() {
                crate::schema_capnp::type_::Which::Void(_) => introspect::TypeVariant::Void,
                crate::schema_capnp::type_::Which::Bool(_) => introspect::TypeVariant::Bool,
                crate::schema_capnp::type_::Which::Int8(_) => introspect::TypeVariant::Int8,
                crate::schema_capnp::type_::Which::Int16(_) => introspect::TypeVariant::Int16,
                crate::schema_capnp::type_::Which::Int32(_) => introspect::TypeVariant::Int32,
                crate::schema_capnp::type_::Which::Int64(_) => introspect::TypeVariant::Int64,
                crate::schema_capnp::type_::Which::Uint8(_) => introspect::TypeVariant::UInt8,
                crate::schema_capnp::type_::Which::Uint16(_) => introspect::TypeVariant::UInt16,
                crate::schema_capnp::type_::Which::Uint32(_) => introspect::TypeVariant::UInt32,
                crate::schema_capnp::type_::Which::Uint64(_) => introspect::TypeVariant::UInt64,
                crate::schema_capnp::type_::Which::Float32(_) => introspect::TypeVariant::Float32,
                crate::schema_capnp::type_::Which::Float64(_) => introspect::TypeVariant::Float64,
                crate::schema_capnp::type_::Which::Text(_) => introspect::TypeVariant::Text,
                crate::schema_capnp::type_::Which::Data(_) => introspect::TypeVariant::Data,
                crate::schema_capnp::type_::Which::List(_) => {
                    todo!();
                }
                crate::schema_capnp::type_::Which::Enum(s) => TypeVariant::Enum(RawEnumSchema {
                    encoded_node: &[],
                    annotation_types: dynamic_annotation_marker,
                }),
                crate::schema_capnp::type_::Which::Struct(_) => {
                    todo!();
                }
                crate::schema_capnp::type_::Which::Interface(_) => {
                    TypeVariant::Capability(RawCapabilitySchema { encoded_node: &[] })
                }
                crate::schema_capnp::type_::Which::AnyPointer(_) => {
                    introspect::TypeVariant::AnyPointer
                }
            }
            .into()
        } else {
            (self.parent.raw.field_types)(self.index)
        }
    }

    pub fn get_index(&self) -> u16 {
        self.index
    }

    pub fn get_annotations(self) -> Result<AnnotationList> {
        Ok(AnnotationList {
            annotations: self.proto.get_annotations()?,
            child_index: Some(self.index),
            get_annotation_type: self.parent.raw.annotation_types,
        })
    }
}

/// A list of fields of a struct, with generics applied.
#[derive(Clone, Copy)]
pub struct FieldList {
    pub(crate) fields: crate::struct_list::Reader<'static, field::Owned>,
    pub(crate) parent: StructSchema,
}

impl FieldList {
    pub fn len(&self) -> u16 {
        self.fields.len() as u16
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    pub fn get(self, index: u16) -> Field {
        Field {
            proto: self.fields.get(index as u32),
            index,
            parent: self.parent,
        }
    }

    pub fn iter(self) -> ShortListIter<Self, Field> {
        ShortListIter::new(self, self.len())
    }
}

impl IndexMove<u16, Field> for FieldList {
    fn index_move(&self, index: u16) -> Field {
        self.get(index)
    }
}

impl ::core::iter::IntoIterator for FieldList {
    type Item = Field;
    type IntoIter = ShortListIter<FieldList, Self::Item>;

    fn into_iter(self) -> Self::IntoIter {
        self.iter()
    }
}

/// A list of a subset of fields of a struct, with generics applied.
#[derive(Clone, Copy)]
pub struct FieldSubset {
    fields: struct_list::Reader<'static, field::Owned>,
    indices: &'static [u16],
    parent: StructSchema,
}

impl FieldSubset {
    pub fn len(&self) -> u16 {
        self.indices.len() as u16
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    pub fn get(self, index: u16) -> Field {
        let index = self.indices[index as usize];
        Field {
            proto: self.fields.get(index as u32),
            index,
            parent: self.parent,
        }
    }

    pub fn iter(self) -> ShortListIter<Self, Field> {
        ShortListIter::new(self, self.len())
    }
}

impl IndexMove<u16, Field> for FieldSubset {
    fn index_move(&self, index: u16) -> Field {
        self.get(index)
    }
}

impl ::core::iter::IntoIterator for FieldSubset {
    type Item = Field;
    type IntoIter = ShortListIter<FieldSubset, Self::Item>;

    fn into_iter(self) -> Self::IntoIter {
        self.iter()
    }
}

/// An enum, with generics applied. (Generics may affect types of annotations.)
#[derive(Clone, Copy)]
pub struct EnumSchema {
    pub(crate) raw: RawEnumSchema,
    pub(crate) proto: node::Reader<'static>,
}

impl EnumSchema {
    pub fn new(raw: RawEnumSchema) -> Self {
        let proto = crate::any_pointer::Reader::new(unsafe {
            layout::PointerReader::get_root_unchecked(raw.encoded_node.as_ptr() as *const u8)
        })
        .get_as()
        .unwrap();
        Self { raw, proto }
    }

    pub fn get_proto(self) -> node::Reader<'static> {
        self.proto
    }

    pub fn get_enumerants(self) -> crate::Result<EnumerantList> {
        if let node::Enum(s) = self.proto.which()? {
            Ok(EnumerantList {
                enumerants: s.get_enumerants()?,
                parent: self,
            })
        } else {
            panic!()
        }
    }

    pub fn get_annotations(self) -> Result<AnnotationList> {
        Ok(AnnotationList {
            annotations: self.proto.get_annotations()?,
            child_index: None,
            get_annotation_type: self.raw.annotation_types,
        })
    }
}

impl From<RawEnumSchema> for EnumSchema {
    fn from(re: RawEnumSchema) -> EnumSchema {
        EnumSchema::new(re)
    }
}

/// An enumerant, with generics applied. (Generics may affect types of annotations.)
#[derive(Clone, Copy)]
pub struct Enumerant {
    ordinal: u16,
    parent: EnumSchema,
    proto: enumerant::Reader<'static>,
}

impl Enumerant {
    pub fn get_containing_enum(self) -> EnumSchema {
        self.parent
    }

    pub fn get_ordinal(self) -> u16 {
        self.ordinal
    }

    pub fn get_proto(self) -> enumerant::Reader<'static> {
        self.proto
    }

    pub fn get_annotations(self) -> Result<AnnotationList> {
        Ok(AnnotationList {
            annotations: self.proto.get_annotations()?,
            child_index: Some(self.ordinal),
            get_annotation_type: self.parent.raw.annotation_types,
        })
    }
}

/// A list of enumerants.
#[derive(Clone, Copy)]
pub struct EnumerantList {
    enumerants: struct_list::Reader<'static, enumerant::Owned>,
    parent: EnumSchema,
}

impl EnumerantList {
    pub fn len(&self) -> u16 {
        self.enumerants.len() as u16
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    pub fn get(self, ordinal: u16) -> Enumerant {
        Enumerant {
            proto: self.enumerants.get(ordinal as u32),
            ordinal,
            parent: self.parent,
        }
    }

    pub fn iter(self) -> ShortListIter<Self, Enumerant> {
        ShortListIter::new(self, self.len())
    }
}

impl IndexMove<u16, Enumerant> for EnumerantList {
    fn index_move(&self, index: u16) -> Enumerant {
        self.get(index)
    }
}

impl ::core::iter::IntoIterator for EnumerantList {
    type Item = Enumerant;
    type IntoIter = ShortListIter<Self, Self::Item>;

    fn into_iter(self) -> Self::IntoIter {
        self.iter()
    }
}

/// An annotation.
#[derive(Clone, Copy)]
pub struct Annotation {
    proto: annotation::Reader<'static>,
    ty: introspect::Type,
}

impl Annotation {
    /// Gets the value held in this annotation.
    pub fn get_value(self) -> Result<dynamic_value::Reader<'static>> {
        dynamic_value::Reader::new(self.proto.get_value()?, self.ty)
    }

    /// Gets the ID of the annotation node.
    pub fn get_id(&self) -> u64 {
        self.proto.get_id()
    }

    /// Gets the type of the value held in this annotation.
    pub fn get_type(&self) -> introspect::Type {
        self.ty
    }
}

/// A list of annotations.
#[derive(Clone, Copy)]
pub struct AnnotationList {
    annotations: struct_list::Reader<'static, annotation::Owned>,
    child_index: Option<u16>,
    get_annotation_type: fn(Option<u16>, u32) -> introspect::Type,
}

impl AnnotationList {
    pub fn len(&self) -> u32 {
        self.annotations.len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    pub fn get(self, index: u32) -> Annotation {
        let proto = self.annotations.get(index);
        #[allow(clippy::fn_address_comparisons)]
        let ty = if self.get_annotation_type != dynamic_annotation_marker {
            (self.get_annotation_type)(self.child_index, index)
        } else {
            match proto.get_value().unwrap().which().unwrap() {
                crate::schema_capnp::value::Which::Void(_) => introspect::TypeVariant::Void,
                crate::schema_capnp::value::Which::Bool(_) => introspect::TypeVariant::Bool,
                crate::schema_capnp::value::Which::Int8(_) => introspect::TypeVariant::Int8,
                crate::schema_capnp::value::Which::Int16(_) => introspect::TypeVariant::Int16,
                crate::schema_capnp::value::Which::Int32(_) => introspect::TypeVariant::Int32,
                crate::schema_capnp::value::Which::Int64(_) => introspect::TypeVariant::Int64,
                crate::schema_capnp::value::Which::Uint8(_) => introspect::TypeVariant::UInt8,
                crate::schema_capnp::value::Which::Uint16(_) => introspect::TypeVariant::UInt16,
                crate::schema_capnp::value::Which::Uint32(_) => introspect::TypeVariant::UInt32,
                crate::schema_capnp::value::Which::Uint64(_) => introspect::TypeVariant::UInt64,
                crate::schema_capnp::value::Which::Float32(_) => introspect::TypeVariant::Float32,
                crate::schema_capnp::value::Which::Float64(_) => introspect::TypeVariant::Float64,
                crate::schema_capnp::value::Which::Text(_) => introspect::TypeVariant::Text,
                crate::schema_capnp::value::Which::Data(_) => introspect::TypeVariant::Data,
                crate::schema_capnp::value::Which::List(_) => {
                    todo!();
                }
                crate::schema_capnp::value::Which::Enum(_) => TypeVariant::Enum(RawEnumSchema {
                    encoded_node: &[],
                    annotation_types: dynamic_annotation_marker,
                }),
                crate::schema_capnp::value::Which::Struct(_) => {
                    todo!();
                }
                crate::schema_capnp::value::Which::Interface(_) => {
                    TypeVariant::Capability(RawCapabilitySchema { encoded_node: &[] })
                }
                crate::schema_capnp::value::Which::AnyPointer(_) => {
                    introspect::TypeVariant::AnyPointer
                }
            }
            .into()
        };

        Annotation { proto, ty }
    }

    /// Returns the first annotation in the list that matches `id`.
    /// Otherwise returns `None`.
    pub fn find(self, id: u64) -> Option<Annotation> {
        self.iter().find(|&annotation| annotation.get_id() == id)
    }

    pub fn iter(self) -> ListIter<Self, Annotation> {
        ListIter::new(self, self.len())
    }
}

impl IndexMove<u32, Annotation> for AnnotationList {
    fn index_move(&self, index: u32) -> Annotation {
        self.get(index)
    }
}

impl ::core::iter::IntoIterator for AnnotationList {
    type Item = Annotation;
    type IntoIter = ListIter<Self, Self::Item>;

    fn into_iter(self) -> Self::IntoIter {
        self.iter()
    }
}

/// A capability schema
#[derive(Clone, Copy)]
pub struct CapabilitySchema {
    pub(crate) _raw: RawCapabilitySchema,
    pub(crate) proto: node::Reader<'static>,
}

impl CapabilitySchema {
    pub fn new(raw: RawCapabilitySchema) -> Self {
        let proto = crate::any_pointer::Reader::new(unsafe {
            layout::PointerReader::get_root_unchecked(raw.encoded_node.as_ptr() as *const u8)
        })
        .get_as()
        .unwrap();
        Self { _raw: raw, proto }
    }

    pub fn get_proto(self) -> node::Reader<'static> {
        self.proto
    }

    pub fn get_methods(self) -> Result<()> {
        todo!();
    }
}

impl From<RawCapabilitySchema> for CapabilitySchema {
    fn from(re: RawCapabilitySchema) -> CapabilitySchema {
        CapabilitySchema::new(re)
    }
}
