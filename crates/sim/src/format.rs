//! Numbers as data variables show them and as data inputs write them.

use degus_controls::{DataType, InputDataType};

use crate::vp::VpSpace;

/// The guide caps integer plus decimal digits at 20.
const MAX_DIGITS: u8 = 20;

/// A value read from VPs.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Number {
    Int(i128),
    Float(f64),
}

/// Reads a data variable's value at `vp`.
pub fn read_number(vp: &VpSpace, at: u16, data_type: DataType) -> Number {
    use Number::{Float, Int};
    match data_type {
        DataType::I16 => Int((vp.get(at) as i16).into()),
        DataType::I32 => Int((vp.u32(at) as i32).into()),
        DataType::HighByte => Int(vp.high(at).into()),
        DataType::LowByte => Int(vp.low(at).into()),
        DataType::I64 => Int((vp.u64(at) as i64).into()),
        DataType::U16 => Int(vp.get(at).into()),
        DataType::U32 => Int(vp.u32(at).into()),
        DataType::F32 => Float(f32::from_bits(vp.u32(at)).into()),
        DataType::F64 => Float(f64::from_bits(vp.u64(at))),
    }
}

/// Formats `number` with `decimal_digits` after the point. Integers are
/// fixed point: 235 with one decimal digit is `23.5`. Only the lowest
/// `integer_digits` digits of the integer part are shown, padded with zeros
/// when `leading_zeros` is set.
// ASSUME: digits that do not fit are cut from the left, and a value that
// is not finite shows as `-`.
pub fn format_number(
    number: Number,
    integer_digits: u8,
    decimal_digits: u8,
    leading_zeros: bool,
) -> String {
    let n = integer_digits.min(MAX_DIGITS);
    let d = decimal_digits.min(MAX_DIGITS - n);
    let scale = 10u128.pow(d.into());
    let (negative, magnitude) = match number {
        Number::Int(v) => (v < 0, v.unsigned_abs()),
        Number::Float(v) if v.is_finite() => (v < 0.0, (v.abs() * scale as f64).round() as u128),
        Number::Float(_) => return "-".to_owned(),
    };
    let integer = (magnitude / scale) % 10u128.pow(n.into());
    let fraction = magnitude % scale;
    let mut text = String::new();
    if negative && magnitude != 0 {
        text.push('-');
    }
    if leading_zeros {
        text.push_str(&format!("{integer:0width$}", width = usize::from(n)));
    } else {
        text.push_str(&integer.to_string());
    }
    if d > 0 {
        text.push_str(&format!(".{fraction:0width$}", width = usize::from(d)));
    }
    text
}

/// Characters of the field a data variable aligns its number in.
pub fn field_chars(integer_digits: u8, decimal_digits: u8) -> usize {
    let n = usize::from(integer_digits.min(MAX_DIGITS));
    let d = usize::from(decimal_digits.min(MAX_DIGITS - integer_digits.min(MAX_DIGITS)));
    n + if d > 0 { d + 1 } else { 0 }
}

/// Parses what was typed into a data input as a fixed-point integer with
/// `decimal_digits` decimals: `12.5` with 2 is 1250. `None` when no digit
/// was typed.
pub fn parse_input(text: &str, decimal_digits: u8) -> Option<i128> {
    let (negative, digits) = match text.strip_prefix('-') {
        Some(rest) => (true, rest),
        None => (false, text),
    };
    let (integer, fraction) = digits.split_once('.').unwrap_or((digits, ""));
    if integer.is_empty() && fraction.is_empty() {
        return None;
    }
    let d = usize::from(decimal_digits.min(MAX_DIGITS));
    let mut fraction: String = fraction.chars().take(d).collect();
    while fraction.len() < d {
        fraction.push('0');
    }
    let value: i128 = format!("0{integer}{fraction}").parse().ok()?;
    Some(if negative { -value } else { value })
}

/// What a data input writes to its VP.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InputWrite {
    Words(Vec<u16>),
    HighByte(u8),
    LowByte(u8),
}

/// The words of a fixed-point input `value`, or `None` when the type cannot
/// hold it.
// ASSUME: a value the type cannot hold is rejected like an out-of-limits
// one, and a float input writes the number itself, not the fixed-point
// integer.
pub fn input_write(
    value: i128,
    data_type: InputDataType,
    decimal_digits: u8,
) -> Option<InputWrite> {
    let words = |bits: u64, n: usize| -> Vec<u16> {
        (0..n).rev().map(|i| (bits >> (16 * i)) as u16).collect()
    };
    Some(match data_type {
        InputDataType::I16 => InputWrite::Words(vec![i16::try_from(value).ok()? as u16]),
        InputDataType::I32 => InputWrite::Words(words(i32::try_from(value).ok()? as u32 as u64, 2)),
        InputDataType::HighByte => InputWrite::HighByte(u8::try_from(value).ok()?),
        InputDataType::LowByte => InputWrite::LowByte(u8::try_from(value).ok()?),
        InputDataType::I64 => InputWrite::Words(words(i64::try_from(value).ok()? as u64, 4)),
        InputDataType::Float => {
            let scale = 10f64.powi(i32::from(decimal_digits.min(MAX_DIGITS)));
            let float = (value as f64 / scale) as f32;
            InputWrite::Words(words(float.to_bits().into(), 2))
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats_fixed_point_numbers() {
        use Number::{Float, Int};
        let cases = [
            (Int(235), 3, 1, false, "23.5"),
            (Int(5), 3, 1, false, "0.5"),
            (Int(5), 3, 1, true, "000.5"),
            (Int(-235), 3, 1, false, "-23.5"),
            (Int(42), 2, 0, false, "42"),
            (Int(1234), 2, 0, false, "34"),
            (Int(7), 4, 0, true, "0007"),
            (Int(0), 2, 0, false, "0"),
            (Int(-1), 2, 2, false, "-0.01"),
            (Float(99.0), 3, 2, false, "99.00"),
            (Float(-1.25), 2, 1, false, "-1.3"),
            (Float(f64::NAN), 2, 1, false, "-"),
        ];
        for (number, n, d, zeros, text) in cases {
            assert_eq!(
                format_number(number, n, d, zeros),
                text,
                "{number:?} {n}.{d}"
            );
        }
        assert_eq!(field_chars(3, 1), 5);
        assert_eq!(field_chars(2, 0), 2);
    }

    #[test]
    fn reads_every_data_type() {
        let mut vp = VpSpace::new();
        vp.write(0x1000, &[0xFFFE, 0x0001, 0x0000, 0x0000]).unwrap();
        assert_eq!(read_number(&vp, 0x1000, DataType::I16), Number::Int(-2));
        assert_eq!(read_number(&vp, 0x1000, DataType::U16), Number::Int(0xFFFE));
        assert_eq!(
            read_number(&vp, 0x1000, DataType::HighByte),
            Number::Int(0xFF)
        );
        assert_eq!(
            read_number(&vp, 0x1000, DataType::LowByte),
            Number::Int(0xFE)
        );
        assert_eq!(
            read_number(&vp, 0x1000, DataType::I32),
            Number::Int(-131_071)
        );
        assert_eq!(
            read_number(&vp, 0x1000, DataType::U32),
            Number::Int(0xFFFE_0001)
        );
        vp.write(0x2000, &[0x42C6, 0x0000]).unwrap();
        assert_eq!(read_number(&vp, 0x2000, DataType::F32), Number::Float(99.0));
    }

    #[test]
    fn parses_typed_input_as_fixed_point() {
        assert_eq!(parse_input("12.5", 2), Some(1250));
        assert_eq!(parse_input("12", 1), Some(120));
        assert_eq!(parse_input("-3.14159", 2), Some(-314));
        assert_eq!(parse_input(".5", 1), Some(5));
        assert_eq!(parse_input("", 1), None);
        assert_eq!(parse_input("-", 0), None);
    }

    #[test]
    fn writes_input_values_by_type() {
        use InputDataType::*;
        assert_eq!(
            input_write(-2, I16, 0),
            Some(InputWrite::Words(vec![0xFFFE]))
        );
        assert_eq!(input_write(40_000, I16, 0), None);
        assert_eq!(
            input_write(70_000, I32, 0),
            Some(InputWrite::Words(vec![0x0001, 0x1170]))
        );
        assert_eq!(input_write(200, LowByte, 0), Some(InputWrite::LowByte(200)));
        assert_eq!(input_write(300, HighByte, 0), None);
        assert_eq!(
            input_write(990, Float, 1),
            Some(InputWrite::Words(vec![0x42C6, 0x0000]))
        );
        assert_eq!(
            input_write(-1, I64, 0),
            Some(InputWrite::Words(vec![0xFFFF; 4]))
        );
    }
}
