// This file is generated by rust-protobuf 2.2.4. Do not edit
// @generated

// https://github.com/Manishearth/rust-clippy/issues/702
#![allow(unknown_lints)]
#![allow(clippy)]

#![cfg_attr(rustfmt, rustfmt_skip)]

#![allow(box_pointers)]
#![allow(dead_code)]
#![allow(missing_docs)]
#![allow(non_camel_case_types)]
#![allow(non_snake_case)]
#![allow(non_upper_case_globals)]
#![allow(trivial_casts)]
#![allow(unsafe_code)]
#![allow(unused_imports)]
#![allow(unused_results)]

use protobuf::Message as Message_imported_for_functions;
use protobuf::ProtobufEnum as ProtobufEnum_imported_for_functions;

#[derive(PartialEq,Clone,Default)]
#[cfg_attr(feature = "with-serde", derive(Serialize, Deserialize))]
pub struct Transaction {
    // message fields
    originator: ::protobuf::SingularField<::std::string::String>,
    destination: ::protobuf::SingularField<::std::string::String>,
    amount: ::std::option::Option<u64>,
    nonce: ::std::option::Option<u64>,
    method_name: ::protobuf::SingularField<::std::string::String>,
    args: ::protobuf::SingularField<::std::vec::Vec<u8>>,
    signature: ::protobuf::SingularField<::std::vec::Vec<u8>>,
    // special fields
    #[cfg_attr(feature = "with-serde", serde(skip))]
    pub unknown_fields: ::protobuf::UnknownFields,
    #[cfg_attr(feature = "with-serde", serde(skip))]
    pub cached_size: ::protobuf::CachedSize,
}

impl Transaction {
    pub fn new() -> Transaction {
        ::std::default::Default::default()
    }

    // required string originator = 1;

    pub fn clear_originator(&mut self) {
        self.originator.clear();
    }

    pub fn has_originator(&self) -> bool {
        self.originator.is_some()
    }

    // Param is passed by value, moved
    pub fn set_originator(&mut self, v: ::std::string::String) {
        self.originator = ::protobuf::SingularField::some(v);
    }

    // Mutable pointer to the field.
    // If field is not initialized, it is initialized with default value first.
    pub fn mut_originator(&mut self) -> &mut ::std::string::String {
        if self.originator.is_none() {
            self.originator.set_default();
        }
        self.originator.as_mut().unwrap()
    }

    // Take field
    pub fn take_originator(&mut self) -> ::std::string::String {
        self.originator.take().unwrap_or_else(|| ::std::string::String::new())
    }

    pub fn get_originator(&self) -> &str {
        match self.originator.as_ref() {
            Some(v) => &v,
            None => "",
        }
    }

    // required string destination = 2;

    pub fn clear_destination(&mut self) {
        self.destination.clear();
    }

    pub fn has_destination(&self) -> bool {
        self.destination.is_some()
    }

    // Param is passed by value, moved
    pub fn set_destination(&mut self, v: ::std::string::String) {
        self.destination = ::protobuf::SingularField::some(v);
    }

    // Mutable pointer to the field.
    // If field is not initialized, it is initialized with default value first.
    pub fn mut_destination(&mut self) -> &mut ::std::string::String {
        if self.destination.is_none() {
            self.destination.set_default();
        }
        self.destination.as_mut().unwrap()
    }

    // Take field
    pub fn take_destination(&mut self) -> ::std::string::String {
        self.destination.take().unwrap_or_else(|| ::std::string::String::new())
    }

    pub fn get_destination(&self) -> &str {
        match self.destination.as_ref() {
            Some(v) => &v,
            None => "",
        }
    }

    // required uint64 amount = 3;

    pub fn clear_amount(&mut self) {
        self.amount = ::std::option::Option::None;
    }

    pub fn has_amount(&self) -> bool {
        self.amount.is_some()
    }

    // Param is passed by value, moved
    pub fn set_amount(&mut self, v: u64) {
        self.amount = ::std::option::Option::Some(v);
    }

    pub fn get_amount(&self) -> u64 {
        self.amount.unwrap_or(0)
    }

    // required uint64 nonce = 4;

    pub fn clear_nonce(&mut self) {
        self.nonce = ::std::option::Option::None;
    }

    pub fn has_nonce(&self) -> bool {
        self.nonce.is_some()
    }

    // Param is passed by value, moved
    pub fn set_nonce(&mut self, v: u64) {
        self.nonce = ::std::option::Option::Some(v);
    }

    pub fn get_nonce(&self) -> u64 {
        self.nonce.unwrap_or(0)
    }

    // required string method_name = 5;

    pub fn clear_method_name(&mut self) {
        self.method_name.clear();
    }

    pub fn has_method_name(&self) -> bool {
        self.method_name.is_some()
    }

    // Param is passed by value, moved
    pub fn set_method_name(&mut self, v: ::std::string::String) {
        self.method_name = ::protobuf::SingularField::some(v);
    }

    // Mutable pointer to the field.
    // If field is not initialized, it is initialized with default value first.
    pub fn mut_method_name(&mut self) -> &mut ::std::string::String {
        if self.method_name.is_none() {
            self.method_name.set_default();
        }
        self.method_name.as_mut().unwrap()
    }

    // Take field
    pub fn take_method_name(&mut self) -> ::std::string::String {
        self.method_name.take().unwrap_or_else(|| ::std::string::String::new())
    }

    pub fn get_method_name(&self) -> &str {
        match self.method_name.as_ref() {
            Some(v) => &v,
            None => "",
        }
    }

    // required bytes args = 6;

    pub fn clear_args(&mut self) {
        self.args.clear();
    }

    pub fn has_args(&self) -> bool {
        self.args.is_some()
    }

    // Param is passed by value, moved
    pub fn set_args(&mut self, v: ::std::vec::Vec<u8>) {
        self.args = ::protobuf::SingularField::some(v);
    }

    // Mutable pointer to the field.
    // If field is not initialized, it is initialized with default value first.
    pub fn mut_args(&mut self) -> &mut ::std::vec::Vec<u8> {
        if self.args.is_none() {
            self.args.set_default();
        }
        self.args.as_mut().unwrap()
    }

    // Take field
    pub fn take_args(&mut self) -> ::std::vec::Vec<u8> {
        self.args.take().unwrap_or_else(|| ::std::vec::Vec::new())
    }

    pub fn get_args(&self) -> &[u8] {
        match self.args.as_ref() {
            Some(v) => &v,
            None => &[],
        }
    }

    // required bytes signature = 7;

    pub fn clear_signature(&mut self) {
        self.signature.clear();
    }

    pub fn has_signature(&self) -> bool {
        self.signature.is_some()
    }

    // Param is passed by value, moved
    pub fn set_signature(&mut self, v: ::std::vec::Vec<u8>) {
        self.signature = ::protobuf::SingularField::some(v);
    }

    // Mutable pointer to the field.
    // If field is not initialized, it is initialized with default value first.
    pub fn mut_signature(&mut self) -> &mut ::std::vec::Vec<u8> {
        if self.signature.is_none() {
            self.signature.set_default();
        }
        self.signature.as_mut().unwrap()
    }

    // Take field
    pub fn take_signature(&mut self) -> ::std::vec::Vec<u8> {
        self.signature.take().unwrap_or_else(|| ::std::vec::Vec::new())
    }

    pub fn get_signature(&self) -> &[u8] {
        match self.signature.as_ref() {
            Some(v) => &v,
            None => &[],
        }
    }
}

impl ::protobuf::Message for Transaction {
    fn is_initialized(&self) -> bool {
        if self.originator.is_none() {
            return false;
        }
        if self.destination.is_none() {
            return false;
        }
        if self.amount.is_none() {
            return false;
        }
        if self.nonce.is_none() {
            return false;
        }
        if self.method_name.is_none() {
            return false;
        }
        if self.args.is_none() {
            return false;
        }
        if self.signature.is_none() {
            return false;
        }
        true
    }

    fn merge_from(&mut self, is: &mut ::protobuf::CodedInputStream) -> ::protobuf::ProtobufResult<()> {
        while !is.eof()? {
            let (field_number, wire_type) = is.read_tag_unpack()?;
            match field_number {
                1 => {
                    ::protobuf::rt::read_singular_string_into(wire_type, is, &mut self.originator)?;
                },
                2 => {
                    ::protobuf::rt::read_singular_string_into(wire_type, is, &mut self.destination)?;
                },
                3 => {
                    if wire_type != ::protobuf::wire_format::WireTypeVarint {
                        return ::std::result::Result::Err(::protobuf::rt::unexpected_wire_type(wire_type));
                    }
                    let tmp = is.read_uint64()?;
                    self.amount = ::std::option::Option::Some(tmp);
                },
                4 => {
                    if wire_type != ::protobuf::wire_format::WireTypeVarint {
                        return ::std::result::Result::Err(::protobuf::rt::unexpected_wire_type(wire_type));
                    }
                    let tmp = is.read_uint64()?;
                    self.nonce = ::std::option::Option::Some(tmp);
                },
                5 => {
                    ::protobuf::rt::read_singular_string_into(wire_type, is, &mut self.method_name)?;
                },
                6 => {
                    ::protobuf::rt::read_singular_bytes_into(wire_type, is, &mut self.args)?;
                },
                7 => {
                    ::protobuf::rt::read_singular_bytes_into(wire_type, is, &mut self.signature)?;
                },
                _ => {
                    ::protobuf::rt::read_unknown_or_skip_group(field_number, wire_type, is, self.mut_unknown_fields())?;
                },
            };
        }
        ::std::result::Result::Ok(())
    }

    // Compute sizes of nested messages
    #[allow(unused_variables)]
    fn compute_size(&self) -> u32 {
        let mut my_size = 0;
        if let Some(ref v) = self.originator.as_ref() {
            my_size += ::protobuf::rt::string_size(1, &v);
        }
        if let Some(ref v) = self.destination.as_ref() {
            my_size += ::protobuf::rt::string_size(2, &v);
        }
        if let Some(v) = self.amount {
            my_size += ::protobuf::rt::value_size(3, v, ::protobuf::wire_format::WireTypeVarint);
        }
        if let Some(v) = self.nonce {
            my_size += ::protobuf::rt::value_size(4, v, ::protobuf::wire_format::WireTypeVarint);
        }
        if let Some(ref v) = self.method_name.as_ref() {
            my_size += ::protobuf::rt::string_size(5, &v);
        }
        if let Some(ref v) = self.args.as_ref() {
            my_size += ::protobuf::rt::bytes_size(6, &v);
        }
        if let Some(ref v) = self.signature.as_ref() {
            my_size += ::protobuf::rt::bytes_size(7, &v);
        }
        my_size += ::protobuf::rt::unknown_fields_size(self.get_unknown_fields());
        self.cached_size.set(my_size);
        my_size
    }

    fn write_to_with_cached_sizes(&self, os: &mut ::protobuf::CodedOutputStream) -> ::protobuf::ProtobufResult<()> {
        if let Some(ref v) = self.originator.as_ref() {
            os.write_string(1, &v)?;
        }
        if let Some(ref v) = self.destination.as_ref() {
            os.write_string(2, &v)?;
        }
        if let Some(v) = self.amount {
            os.write_uint64(3, v)?;
        }
        if let Some(v) = self.nonce {
            os.write_uint64(4, v)?;
        }
        if let Some(ref v) = self.method_name.as_ref() {
            os.write_string(5, &v)?;
        }
        if let Some(ref v) = self.args.as_ref() {
            os.write_bytes(6, &v)?;
        }
        if let Some(ref v) = self.signature.as_ref() {
            os.write_bytes(7, &v)?;
        }
        os.write_unknown_fields(self.get_unknown_fields())?;
        ::std::result::Result::Ok(())
    }

    fn get_cached_size(&self) -> u32 {
        self.cached_size.get()
    }

    fn get_unknown_fields(&self) -> &::protobuf::UnknownFields {
        &self.unknown_fields
    }

    fn mut_unknown_fields(&mut self) -> &mut ::protobuf::UnknownFields {
        &mut self.unknown_fields
    }

    fn as_any(&self) -> &::std::any::Any {
        self as &::std::any::Any
    }
    fn as_any_mut(&mut self) -> &mut ::std::any::Any {
        self as &mut ::std::any::Any
    }
    fn into_any(self: Box<Self>) -> ::std::boxed::Box<::std::any::Any> {
        self
    }

    fn descriptor(&self) -> &'static ::protobuf::reflect::MessageDescriptor {
        Self::descriptor_static()
    }

    fn new() -> Transaction {
        Transaction::new()
    }

    fn descriptor_static() -> &'static ::protobuf::reflect::MessageDescriptor {
        static mut descriptor: ::protobuf::lazy::Lazy<::protobuf::reflect::MessageDescriptor> = ::protobuf::lazy::Lazy {
            lock: ::protobuf::lazy::ONCE_INIT,
            ptr: 0 as *const ::protobuf::reflect::MessageDescriptor,
        };
        unsafe {
            descriptor.get(|| {
                let mut fields = ::std::vec::Vec::new();
                fields.push(::protobuf::reflect::accessor::make_singular_field_accessor::<_, ::protobuf::types::ProtobufTypeString>(
                    "originator",
                    |m: &Transaction| { &m.originator },
                    |m: &mut Transaction| { &mut m.originator },
                ));
                fields.push(::protobuf::reflect::accessor::make_singular_field_accessor::<_, ::protobuf::types::ProtobufTypeString>(
                    "destination",
                    |m: &Transaction| { &m.destination },
                    |m: &mut Transaction| { &mut m.destination },
                ));
                fields.push(::protobuf::reflect::accessor::make_option_accessor::<_, ::protobuf::types::ProtobufTypeUint64>(
                    "amount",
                    |m: &Transaction| { &m.amount },
                    |m: &mut Transaction| { &mut m.amount },
                ));
                fields.push(::protobuf::reflect::accessor::make_option_accessor::<_, ::protobuf::types::ProtobufTypeUint64>(
                    "nonce",
                    |m: &Transaction| { &m.nonce },
                    |m: &mut Transaction| { &mut m.nonce },
                ));
                fields.push(::protobuf::reflect::accessor::make_singular_field_accessor::<_, ::protobuf::types::ProtobufTypeString>(
                    "method_name",
                    |m: &Transaction| { &m.method_name },
                    |m: &mut Transaction| { &mut m.method_name },
                ));
                fields.push(::protobuf::reflect::accessor::make_singular_field_accessor::<_, ::protobuf::types::ProtobufTypeBytes>(
                    "args",
                    |m: &Transaction| { &m.args },
                    |m: &mut Transaction| { &mut m.args },
                ));
                fields.push(::protobuf::reflect::accessor::make_singular_field_accessor::<_, ::protobuf::types::ProtobufTypeBytes>(
                    "signature",
                    |m: &Transaction| { &m.signature },
                    |m: &mut Transaction| { &mut m.signature },
                ));
                ::protobuf::reflect::MessageDescriptor::new::<Transaction>(
                    "Transaction",
                    fields,
                    file_descriptor_proto()
                )
            })
        }
    }

    fn default_instance() -> &'static Transaction {
        static mut instance: ::protobuf::lazy::Lazy<Transaction> = ::protobuf::lazy::Lazy {
            lock: ::protobuf::lazy::ONCE_INIT,
            ptr: 0 as *const Transaction,
        };
        unsafe {
            instance.get(Transaction::new)
        }
    }
}

impl ::protobuf::Clear for Transaction {
    fn clear(&mut self) {
        self.clear_originator();
        self.clear_destination();
        self.clear_amount();
        self.clear_nonce();
        self.clear_method_name();
        self.clear_args();
        self.clear_signature();
        self.unknown_fields.clear();
    }
}

impl ::std::fmt::Debug for Transaction {
    fn fmt(&self, f: &mut ::std::fmt::Formatter) -> ::std::fmt::Result {
        ::protobuf::text_format::fmt(self, f)
    }
}

impl ::protobuf::reflect::ProtobufValue for Transaction {
    fn as_ref(&self) -> ::protobuf::reflect::ProtobufValueRef {
        ::protobuf::reflect::ProtobufValueRef::Message(self)
    }
}

#[derive(PartialEq,Clone,Default)]
#[cfg_attr(feature = "with-serde", derive(Serialize, Deserialize))]
pub struct ChainPayload {
    // message fields
    transactions: ::protobuf::RepeatedField<::std::vec::Vec<u8>>,
    // special fields
    #[cfg_attr(feature = "with-serde", serde(skip))]
    pub unknown_fields: ::protobuf::UnknownFields,
    #[cfg_attr(feature = "with-serde", serde(skip))]
    pub cached_size: ::protobuf::CachedSize,
}

impl ChainPayload {
    pub fn new() -> ChainPayload {
        ::std::default::Default::default()
    }

    // repeated bytes transactions = 1;

    pub fn clear_transactions(&mut self) {
        self.transactions.clear();
    }

    // Param is passed by value, moved
    pub fn set_transactions(&mut self, v: ::protobuf::RepeatedField<::std::vec::Vec<u8>>) {
        self.transactions = v;
    }

    // Mutable pointer to the field.
    pub fn mut_transactions(&mut self) -> &mut ::protobuf::RepeatedField<::std::vec::Vec<u8>> {
        &mut self.transactions
    }

    // Take field
    pub fn take_transactions(&mut self) -> ::protobuf::RepeatedField<::std::vec::Vec<u8>> {
        ::std::mem::replace(&mut self.transactions, ::protobuf::RepeatedField::new())
    }

    pub fn get_transactions(&self) -> &[::std::vec::Vec<u8>] {
        &self.transactions
    }
}

impl ::protobuf::Message for ChainPayload {
    fn is_initialized(&self) -> bool {
        true
    }

    fn merge_from(&mut self, is: &mut ::protobuf::CodedInputStream) -> ::protobuf::ProtobufResult<()> {
        while !is.eof()? {
            let (field_number, wire_type) = is.read_tag_unpack()?;
            match field_number {
                1 => {
                    ::protobuf::rt::read_repeated_bytes_into(wire_type, is, &mut self.transactions)?;
                },
                _ => {
                    ::protobuf::rt::read_unknown_or_skip_group(field_number, wire_type, is, self.mut_unknown_fields())?;
                },
            };
        }
        ::std::result::Result::Ok(())
    }

    // Compute sizes of nested messages
    #[allow(unused_variables)]
    fn compute_size(&self) -> u32 {
        let mut my_size = 0;
        for value in &self.transactions {
            my_size += ::protobuf::rt::bytes_size(1, &value);
        };
        my_size += ::protobuf::rt::unknown_fields_size(self.get_unknown_fields());
        self.cached_size.set(my_size);
        my_size
    }

    fn write_to_with_cached_sizes(&self, os: &mut ::protobuf::CodedOutputStream) -> ::protobuf::ProtobufResult<()> {
        for v in &self.transactions {
            os.write_bytes(1, &v)?;
        };
        os.write_unknown_fields(self.get_unknown_fields())?;
        ::std::result::Result::Ok(())
    }

    fn get_cached_size(&self) -> u32 {
        self.cached_size.get()
    }

    fn get_unknown_fields(&self) -> &::protobuf::UnknownFields {
        &self.unknown_fields
    }

    fn mut_unknown_fields(&mut self) -> &mut ::protobuf::UnknownFields {
        &mut self.unknown_fields
    }

    fn as_any(&self) -> &::std::any::Any {
        self as &::std::any::Any
    }
    fn as_any_mut(&mut self) -> &mut ::std::any::Any {
        self as &mut ::std::any::Any
    }
    fn into_any(self: Box<Self>) -> ::std::boxed::Box<::std::any::Any> {
        self
    }

    fn descriptor(&self) -> &'static ::protobuf::reflect::MessageDescriptor {
        Self::descriptor_static()
    }

    fn new() -> ChainPayload {
        ChainPayload::new()
    }

    fn descriptor_static() -> &'static ::protobuf::reflect::MessageDescriptor {
        static mut descriptor: ::protobuf::lazy::Lazy<::protobuf::reflect::MessageDescriptor> = ::protobuf::lazy::Lazy {
            lock: ::protobuf::lazy::ONCE_INIT,
            ptr: 0 as *const ::protobuf::reflect::MessageDescriptor,
        };
        unsafe {
            descriptor.get(|| {
                let mut fields = ::std::vec::Vec::new();
                fields.push(::protobuf::reflect::accessor::make_repeated_field_accessor::<_, ::protobuf::types::ProtobufTypeBytes>(
                    "transactions",
                    |m: &ChainPayload| { &m.transactions },
                    |m: &mut ChainPayload| { &mut m.transactions },
                ));
                ::protobuf::reflect::MessageDescriptor::new::<ChainPayload>(
                    "ChainPayload",
                    fields,
                    file_descriptor_proto()
                )
            })
        }
    }

    fn default_instance() -> &'static ChainPayload {
        static mut instance: ::protobuf::lazy::Lazy<ChainPayload> = ::protobuf::lazy::Lazy {
            lock: ::protobuf::lazy::ONCE_INIT,
            ptr: 0 as *const ChainPayload,
        };
        unsafe {
            instance.get(ChainPayload::new)
        }
    }
}

impl ::protobuf::Clear for ChainPayload {
    fn clear(&mut self) {
        self.clear_transactions();
        self.unknown_fields.clear();
    }
}

impl ::std::fmt::Debug for ChainPayload {
    fn fmt(&self, f: &mut ::std::fmt::Formatter) -> ::std::fmt::Result {
        ::protobuf::text_format::fmt(self, f)
    }
}

impl ::protobuf::reflect::ProtobufValue for ChainPayload {
    fn as_ref(&self) -> ::protobuf::reflect::ProtobufValueRef {
        ::protobuf::reflect::ProtobufValueRef::Message(self)
    }
}

static file_descriptor_proto_data: &'static [u8] = b"\
    \n\x11transaction.proto\"\x8b\x01\n\x0bTransaction\x12\x12\n\noriginator\
    \x18\x01\x20\x02(\t\x12\x13\n\x0bdestination\x18\x02\x20\x02(\t\x12\x0e\
    \n\x06amount\x18\x03\x20\x02(\x04\x12\r\n\x05nonce\x18\x04\x20\x02(\x04\
    \x12\x13\n\x0bmethod_name\x18\x05\x20\x02(\t\x12\x0c\n\x04args\x18\x06\
    \x20\x02(\x0c\x12\x11\n\tsignature\x18\x07\x20\x02(\x0c\"$\n\x0cChainPay\
    load\x12\x14\n\x0ctransactions\x18\x01\x20\x03(\x0c\
";

static mut file_descriptor_proto_lazy: ::protobuf::lazy::Lazy<::protobuf::descriptor::FileDescriptorProto> = ::protobuf::lazy::Lazy {
    lock: ::protobuf::lazy::ONCE_INIT,
    ptr: 0 as *const ::protobuf::descriptor::FileDescriptorProto,
};

fn parse_descriptor_proto() -> ::protobuf::descriptor::FileDescriptorProto {
    ::protobuf::parse_from_bytes(file_descriptor_proto_data).unwrap()
}

pub fn file_descriptor_proto() -> &'static ::protobuf::descriptor::FileDescriptorProto {
    unsafe {
        file_descriptor_proto_lazy.get(|| {
            parse_descriptor_proto()
        })
    }
}
