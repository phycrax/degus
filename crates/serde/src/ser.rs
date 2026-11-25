//! Values into bytes.

use core::fmt;

use serde::ser::{self, Impossible, Serialize};

use crate::EncodeError;

/// Where the serializer puts the bytes.
pub(crate) trait Output {
    fn extend(&mut self, bytes: &[u8]) -> Result<(), EncodeError>;
}

/// Bytes written into a slice, from its start on.
pub(crate) struct Slice<'b> {
    buf: &'b mut [u8],
    len: usize,
}

impl<'b> Slice<'b> {
    pub(crate) fn new(buf: &'b mut [u8]) -> Self {
        Self { buf, len: 0 }
    }

    /// The bytes written.
    pub(crate) fn finish(self) -> &'b mut [u8] {
        &mut self.buf[..self.len]
    }
}

impl Output for Slice<'_> {
    #[inline(never)]
    fn extend(&mut self, bytes: &[u8]) -> Result<(), EncodeError> {
        let end = self.len + bytes.len();
        let slot = self
            .buf
            .get_mut(self.len..end)
            .ok_or(EncodeError::BufferFull)?;
        slot.copy_from_slice(bytes);
        self.len = end;
        Ok(())
    }
}

/// Counts the bytes instead of keeping them.
pub(crate) struct Count(pub(crate) usize);

impl Output for Count {
    fn extend(&mut self, bytes: &[u8]) -> Result<(), EncodeError> {
        self.0 += bytes.len();
        Ok(())
    }
}

pub(crate) struct Serializer<O> {
    pub(crate) output: O,
}

/// Serializer methods that write a number's big-endian bytes.
macro_rules! big_endian {
    ($($method:ident: $ty:ty),* $(,)?) => {$(
        fn $method(self, v: $ty) -> Result<(), EncodeError> {
            self.output.extend(&v.to_be_bytes())
        }
    )*};
}

impl<O: Output> ser::Serializer for &mut Serializer<O> {
    type Ok = ();
    type Error = EncodeError;

    type SerializeSeq = Self;
    type SerializeTuple = Self;
    type SerializeTupleStruct = Self;
    type SerializeStruct = Self;
    type SerializeTupleVariant = Impossible<(), EncodeError>;
    type SerializeMap = Impossible<(), EncodeError>;
    type SerializeStructVariant = Impossible<(), EncodeError>;

    fn is_human_readable(&self) -> bool {
        false
    }

    big_endian! {
        serialize_i8: i8, serialize_i16: i16, serialize_i32: i32,
        serialize_i64: i64, serialize_i128: i128,
        serialize_u8: u8, serialize_u16: u16, serialize_u32: u32,
        serialize_u64: u64, serialize_u128: u128,
        serialize_f32: f32, serialize_f64: f64,
    }

    /// A word, 0 or 1.
    fn serialize_bool(self, v: bool) -> Result<(), EncodeError> {
        self.serialize_u16(v.into())
    }

    fn serialize_char(self, v: char) -> Result<(), EncodeError> {
        self.output.extend(v.encode_utf8(&mut [0; 4]).as_bytes())
    }

    fn serialize_str(self, v: &str) -> Result<(), EncodeError> {
        self.output.extend(v.as_bytes())
    }

    fn serialize_bytes(self, v: &[u8]) -> Result<(), EncodeError> {
        self.output.extend(v)
    }

    /// Writes the text as [`serialize_str`](Self::serialize_str) would,
    /// without a buffer for it.
    fn collect_str<T: fmt::Display + ?Sized>(self, value: &T) -> Result<(), EncodeError> {
        struct Text<'a, O> {
            output: &'a mut O,
            error: Option<EncodeError>,
        }
        impl<O: Output> fmt::Write for Text<'_, O> {
            fn write_str(&mut self, s: &str) -> fmt::Result {
                self.output.extend(s.as_bytes()).map_err(|error| {
                    self.error = Some(error);
                    fmt::Error
                })
            }
        }
        let mut text = Text {
            output: &mut self.output,
            error: None,
        };
        // An error without one of ours is the value's own `Display` failing.
        fmt::write(&mut text, format_args!("{value}"))
            .map_err(|fmt::Error| text.error.unwrap_or(EncodeError::Custom))
    }

    fn serialize_none(self) -> Result<(), EncodeError> {
        Err(EncodeError::Unsupported)
    }

    fn serialize_some<T: Serialize + ?Sized>(self, _value: &T) -> Result<(), EncodeError> {
        Err(EncodeError::Unsupported)
    }

    fn serialize_unit(self) -> Result<(), EncodeError> {
        Ok(())
    }

    fn serialize_unit_struct(self, _name: &'static str) -> Result<(), EncodeError> {
        Ok(())
    }

    /// The variant's index, as a word.
    fn serialize_unit_variant(
        self,
        _name: &'static str,
        variant_index: u32,
        _variant: &'static str,
    ) -> Result<(), EncodeError> {
        let index = u16::try_from(variant_index).map_err(|_| EncodeError::BadEnum)?;
        self.serialize_u16(index)
    }

    fn serialize_newtype_struct<T: Serialize + ?Sized>(
        self,
        _name: &'static str,
        value: &T,
    ) -> Result<(), EncodeError> {
        value.serialize(self)
    }

    fn serialize_newtype_variant<T: Serialize + ?Sized>(
        self,
        _name: &'static str,
        _variant_index: u32,
        _variant: &'static str,
        _value: &T,
    ) -> Result<(), EncodeError> {
        Err(EncodeError::Unsupported)
    }

    /// The elements, without their number.
    fn serialize_seq(self, _len: Option<usize>) -> Result<Self, EncodeError> {
        Ok(self)
    }

    fn serialize_tuple(self, _len: usize) -> Result<Self, EncodeError> {
        Ok(self)
    }

    fn serialize_tuple_struct(self, _name: &'static str, _len: usize) -> Result<Self, EncodeError> {
        Ok(self)
    }

    fn serialize_tuple_variant(
        self,
        _name: &'static str,
        _variant_index: u32,
        _variant: &'static str,
        _len: usize,
    ) -> Result<Self::SerializeTupleVariant, EncodeError> {
        Err(EncodeError::Unsupported)
    }

    fn serialize_map(self, _len: Option<usize>) -> Result<Self::SerializeMap, EncodeError> {
        Err(EncodeError::Unsupported)
    }

    fn serialize_struct(self, _name: &'static str, _len: usize) -> Result<Self, EncodeError> {
        Ok(self)
    }

    fn serialize_struct_variant(
        self,
        _name: &'static str,
        _variant_index: u32,
        _variant: &'static str,
        _len: usize,
    ) -> Result<Self::SerializeStructVariant, EncodeError> {
        Err(EncodeError::Unsupported)
    }
}

impl<O: Output> ser::SerializeSeq for &mut Serializer<O> {
    type Ok = ();
    type Error = EncodeError;

    fn serialize_element<T: Serialize + ?Sized>(&mut self, value: &T) -> Result<(), EncodeError> {
        value.serialize(&mut **self)
    }

    fn end(self) -> Result<(), EncodeError> {
        Ok(())
    }
}

impl<O: Output> ser::SerializeTuple for &mut Serializer<O> {
    type Ok = ();
    type Error = EncodeError;

    fn serialize_element<T: Serialize + ?Sized>(&mut self, value: &T) -> Result<(), EncodeError> {
        value.serialize(&mut **self)
    }

    fn end(self) -> Result<(), EncodeError> {
        Ok(())
    }
}

impl<O: Output> ser::SerializeTupleStruct for &mut Serializer<O> {
    type Ok = ();
    type Error = EncodeError;

    fn serialize_field<T: Serialize + ?Sized>(&mut self, value: &T) -> Result<(), EncodeError> {
        value.serialize(&mut **self)
    }

    fn end(self) -> Result<(), EncodeError> {
        Ok(())
    }
}

impl<O: Output> ser::SerializeStruct for &mut Serializer<O> {
    type Ok = ();
    type Error = EncodeError;

    fn serialize_field<T: Serialize + ?Sized>(
        &mut self,
        _key: &'static str,
        value: &T,
    ) -> Result<(), EncodeError> {
        value.serialize(&mut **self)
    }

    /// A field skipped at run time, by `#[serde(skip_serializing_if)]`,
    /// would move the fields after it to other VPs.
    fn skip_field(&mut self, _key: &'static str) -> Result<(), EncodeError> {
        Err(EncodeError::Unsupported)
    }

    fn end(self) -> Result<(), EncodeError> {
        Ok(())
    }
}
