//! Bytes into values.

use serde::de::value::U16Deserializer;
use serde::de::{self, Deserialize, DeserializeSeed, Visitor};

use crate::DecodeError;

/// Takes values off the front of some bytes, one after the other, such as
/// the data of a frame that holds several.
///
/// ```
/// use degus_serde::Deserializer;
///
/// let mut data = Deserializer::new(&[0x00, 0x07, 0x12, 0x34, 0x56, 0x78]);
/// let page: u16 = data.take()?;
/// let count: u32 = data.take()?;
/// assert_eq!((page, count), (7, 0x1234_5678));
/// assert!(data.rest().is_empty());
/// # Ok::<(), degus_serde::DecodeError>(())
/// ```
#[derive(Clone, Debug)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub struct Deserializer<'de> {
    input: &'de [u8],
}

impl<'de> Deserializer<'de> {
    /// Takes values from the start of `input` on.
    #[must_use]
    pub const fn new(input: &'de [u8]) -> Self {
        Self { input }
    }

    /// Takes a `T` off the front of the bytes left.
    ///
    /// # Errors
    ///
    /// Any [`DecodeError`] but [`TrailingBytes`](DecodeError::TrailingBytes).
    /// The bytes left are as they were then.
    pub fn take<T: Deserialize<'de>>(&mut self) -> Result<T, DecodeError> {
        let input = self.input;
        let value = T::deserialize(&mut *self);
        if value.is_err() {
            self.input = input;
        }
        value
    }

    /// The bytes not taken yet.
    #[must_use]
    pub const fn rest(&self) -> &'de [u8] {
        self.input
    }

    fn chunk<const N: usize>(&mut self) -> Result<[u8; N], DecodeError> {
        let (bytes, rest) = self
            .input
            .split_first_chunk()
            .ok_or(DecodeError::UnexpectedEnd)?;
        self.input = rest;
        Ok(*bytes)
    }
}

/// Deserializer methods that read a number's big-endian bytes.
macro_rules! big_endian {
    ($($method:ident: $ty:ty => $visit:ident),* $(,)?) => {$(
        fn $method<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, DecodeError> {
            visitor.$visit(<$ty>::from_be_bytes(self.chunk()?))
        }
    )*};
}

/// Deserializer methods for what VP space has no layout for.
macro_rules! unsupported {
    ($($method:ident),* $(,)?) => {$(
        fn $method<V: Visitor<'de>>(self, _visitor: V) -> Result<V::Value, DecodeError> {
            Err(DecodeError::Unsupported)
        }
    )*};
}

impl<'de> de::Deserializer<'de> for &mut Deserializer<'de> {
    type Error = DecodeError;

    fn is_human_readable(&self) -> bool {
        false
    }

    big_endian! {
        deserialize_i8: i8 => visit_i8, deserialize_i16: i16 => visit_i16,
        deserialize_i32: i32 => visit_i32, deserialize_i64: i64 => visit_i64,
        deserialize_i128: i128 => visit_i128,
        deserialize_u8: u8 => visit_u8, deserialize_u16: u16 => visit_u16,
        deserialize_u32: u32 => visit_u32, deserialize_u64: u64 => visit_u64,
        deserialize_u128: u128 => visit_u128,
        deserialize_f32: f32 => visit_f32, deserialize_f64: f64 => visit_f64,
    }

    // The bytes do not say what they hold, or how long a text or a sequence
    // is.
    unsupported! {
        deserialize_any, deserialize_char, deserialize_str, deserialize_string,
        deserialize_bytes, deserialize_byte_buf, deserialize_option,
        deserialize_seq, deserialize_map, deserialize_identifier,
        deserialize_ignored_any,
    }

    /// A word, 0 or 1.
    fn deserialize_bool<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, DecodeError> {
        match u16::from_be_bytes(self.chunk()?) {
            0 => visitor.visit_bool(false),
            1 => visitor.visit_bool(true),
            _ => Err(DecodeError::BadBool),
        }
    }

    fn deserialize_unit<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, DecodeError> {
        visitor.visit_unit()
    }

    fn deserialize_unit_struct<V: Visitor<'de>>(
        self,
        _name: &'static str,
        visitor: V,
    ) -> Result<V::Value, DecodeError> {
        visitor.visit_unit()
    }

    fn deserialize_newtype_struct<V: Visitor<'de>>(
        self,
        _name: &'static str,
        visitor: V,
    ) -> Result<V::Value, DecodeError> {
        visitor.visit_newtype_struct(self)
    }

    fn deserialize_tuple<V: Visitor<'de>>(
        self,
        len: usize,
        visitor: V,
    ) -> Result<V::Value, DecodeError> {
        visitor.visit_seq(Elements {
            de: self,
            left: len,
        })
    }

    fn deserialize_tuple_struct<V: Visitor<'de>>(
        self,
        _name: &'static str,
        len: usize,
        visitor: V,
    ) -> Result<V::Value, DecodeError> {
        self.deserialize_tuple(len, visitor)
    }

    /// The fields in order, as a tuple.
    fn deserialize_struct<V: Visitor<'de>>(
        self,
        _name: &'static str,
        fields: &'static [&'static str],
        visitor: V,
    ) -> Result<V::Value, DecodeError> {
        self.deserialize_tuple(fields.len(), visitor)
    }

    fn deserialize_enum<V: Visitor<'de>>(
        self,
        _name: &'static str,
        _variants: &'static [&'static str],
        visitor: V,
    ) -> Result<V::Value, DecodeError> {
        visitor.visit_enum(self)
    }
}

/// The elements of a tuple, an array or a struct.
struct Elements<'a, 'de> {
    de: &'a mut Deserializer<'de>,
    left: usize,
}

impl<'de> de::SeqAccess<'de> for Elements<'_, 'de> {
    type Error = DecodeError;

    fn next_element_seed<T: DeserializeSeed<'de>>(
        &mut self,
        seed: T,
    ) -> Result<Option<T::Value>, DecodeError> {
        if self.left == 0 {
            return Ok(None);
        }
        self.left -= 1;
        seed.deserialize(&mut *self.de).map(Some)
    }

    fn size_hint(&self) -> Option<usize> {
        Some(self.left)
    }
}

/// The variant's index, as a word.
impl<'de> de::EnumAccess<'de> for &mut Deserializer<'de> {
    type Error = DecodeError;
    type Variant = Self;

    fn variant_seed<V: DeserializeSeed<'de>>(
        self,
        seed: V,
    ) -> Result<(V::Value, Self), DecodeError> {
        let index = u16::from_be_bytes(self.chunk()?);
        let variant = seed.deserialize(U16Deserializer::<DecodeError>::new(index))?;
        Ok((variant, self))
    }
}

/// Only unit variants: the index says nothing of how long a variant's data
/// is.
impl<'de> de::VariantAccess<'de> for &mut Deserializer<'de> {
    type Error = DecodeError;

    fn unit_variant(self) -> Result<(), DecodeError> {
        Ok(())
    }

    fn newtype_variant_seed<T: DeserializeSeed<'de>>(
        self,
        _seed: T,
    ) -> Result<T::Value, DecodeError> {
        Err(DecodeError::Unsupported)
    }

    fn tuple_variant<V: Visitor<'de>>(
        self,
        _len: usize,
        _visitor: V,
    ) -> Result<V::Value, DecodeError> {
        Err(DecodeError::Unsupported)
    }

    fn struct_variant<V: Visitor<'de>>(
        self,
        _fields: &'static [&'static str],
        _visitor: V,
    ) -> Result<V::Value, DecodeError> {
        Err(DecodeError::Unsupported)
    }
}
