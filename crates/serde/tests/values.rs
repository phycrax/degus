//! How values encode and decode, and the system variable examples in DWIN's
//! T5L DGUSII Application Development Guide (§5.1), framed by degus-codec.

use std::collections::BTreeMap;
use std::fmt::Debug;

use degus_codec::{Address, Crc, MAX_FRAME, Request, Response};
use degus_serde::{DecodeError, Deserializer, EncodeError, encoded_len, from_bytes, to_slice};
use hex_literal::hex;
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize, Serializer};

/// `value` encodes to `bytes`, and `bytes` decode to `value`.
fn round_trip<T>(value: T, bytes: &[u8])
where
    T: Serialize + DeserializeOwned + PartialEq + Debug,
{
    let mut buf = [0xCC; 64];
    assert_eq!(&*to_slice(&value, &mut buf).unwrap(), bytes, "{value:?}");
    assert_eq!(encoded_len(&value), Ok(bytes.len()), "{value:?}");
    assert_eq!(from_bytes::<T>(bytes), Ok(value));
}

fn encode<T: Serialize + ?Sized>(value: &T) -> Result<Vec<u8>, EncodeError> {
    let mut buf = [0; 64];
    to_slice(value, &mut buf).map(|bytes| bytes.to_vec())
}

#[test]
fn numbers_are_big_endian() {
    round_trip(0x1234_u16, &hex!("1234"));
    round_trip(-2_i16, &hex!("FFFE"));
    round_trip(0x1234_5678_u32, &hex!("12345678"));
    round_trip(-100_000_i32, &hex!("FFFE7960"));
    round_trip(0x0102_0304_0506_0708_u64, &hex!("0102030405060708"));
    round_trip(-1_i64, &hex!("FFFFFFFFFFFFFFFF"));
    round_trip(1.5_f32, &hex!("3FC00000"));
    round_trip(-2.0_f64, &hex!("C000000000000000"));
    round_trip(0x12_u8, &hex!("12"));
    round_trip(-1_i8, &hex!("FF"));
}

#[derive(Debug, PartialEq, Serialize, Deserialize)]
struct MyData {
    a_msb: u8,
    a_lsb: u8,
    b: u16,
    c: u32,
}

/// dguscard's example: values written from `0x1000` on, each VP holding
/// its own address, then taken back in the same order.
#[test]
fn values_fill_the_vps_one_after_the_other() {
    let data = MyData {
        a_msb: 0x10,
        a_lsb: 0x07,
        b: 0x1008,
        c: 0x1009_100A,
    };
    let values = (
        0x1000_u16,
        0x1001_1002_u32,
        0x1003_1004_1005_1006_u64,
        data,
        0x10_u8,
        0x0B_u8,
    );
    let bytes = hex!("1000 1001 1002 1003 1004 1005 1006 1007 1008 1009 100A 100B");
    assert_eq!(encode(&values).unwrap(), bytes);

    let mut vps = Deserializer::new(&bytes);
    assert_eq!(vps.take::<u16>(), Ok(0x1000));
    assert_eq!(vps.take::<u32>(), Ok(0x1001_1002));
    assert_eq!(vps.take::<u64>(), Ok(0x1003_1004_1005_1006));
    assert_eq!(
        vps.take::<MyData>(),
        Ok(MyData {
            a_msb: 0x10,
            a_lsb: 0x07,
            b: 0x1008,
            c: 0x1009_100A,
        })
    );
    assert_eq!(vps.take::<(u8, u8)>(), Ok((0x10, 0x0B)));
    assert_eq!(vps.rest(), []);
}

#[test]
fn bools_are_words() {
    round_trip(true, &hex!("0001"));
    round_trip(false, &hex!("0000"));
    assert_eq!(from_bytes::<bool>(&hex!("0100")), Err(DecodeError::BadBool));
    assert_eq!(from_bytes::<bool>(&hex!("0002")), Err(DecodeError::BadBool));
}

#[derive(Debug, PartialEq, Serialize, Deserialize)]
enum Speed {
    Off,
    Low,
    Medium,
    High,
}

#[test]
fn unit_variants_are_their_index() {
    round_trip(Speed::Off, &hex!("0000"));
    round_trip(Speed::Medium, &hex!("0002"));
    round_trip(Speed::High, &hex!("0003"));
    assert_eq!(
        from_bytes::<Speed>(&hex!("0004")),
        Err(DecodeError::Invalid)
    );
}

#[test]
fn variant_indexes_fit_a_word() {
    struct Far;
    impl Serialize for Far {
        fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
            serializer.serialize_unit_variant("Far", 0x1_0000, "Far")
        }
    }
    assert_eq!(encode(&Far), Err(EncodeError::BadEnum));
}

#[derive(Debug, PartialEq, Serialize, Deserialize)]
struct Page(u16);

#[derive(Debug, PartialEq, Serialize, Deserialize)]
struct Nothing;

#[test]
fn newtypes_are_what_they_hold_and_units_are_nothing() {
    round_trip(Page(7), &hex!("0007"));
    round_trip((), &[]);
    round_trip(Nothing, &[]);
    round_trip((Nothing, Page(1), ()), &hex!("0001"));
}

#[test]
fn arrays_are_their_elements() {
    round_trip([0x1234_u16, 0x5678, 0x9ABC], &hex!("1234 5678 9ABC"));
    round_trip([0xDE_u8, 0xAD, 0xBE, 0xEF], &hex!("DEADBEEF"));
    round_trip([Page(1), Page(2)], &hex!("0001 0002"));
}

#[test]
fn sequences_only_encode() {
    assert_eq!(encode(&[1_u16, 2, 3][..]).unwrap(), hex!("0001 0002 0003"));
    assert_eq!(
        encode(&vec![Speed::Low, Speed::High]).unwrap(),
        hex!("0001 0003")
    );
    assert_eq!(
        from_bytes::<Vec<u16>>(&hex!("0001 0002")),
        Err(DecodeError::Unsupported)
    );
}

/// Formats itself, as a version string would.
struct Version(u8, u8);

impl Serialize for Version {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_str(&format_args!("V{}.{}", self.0, self.1))
    }
}

#[test]
fn texts_are_their_utf8_bytes_and_only_encode() {
    assert_eq!(encode("Hi").unwrap(), b"Hi");
    assert_eq!(encode(&'é').unwrap(), hex!("C3A9"));
    assert_eq!(encode(&Version(6, 3)).unwrap(), b"V6.3");
    let mut short = [0; 3];
    assert_eq!(
        to_slice(&Version(6, 3), &mut short),
        Err(EncodeError::BufferFull)
    );
    assert_eq!(from_bytes::<&str>(b"Hi"), Err(DecodeError::Unsupported));
}

#[derive(Debug, PartialEq, Serialize, Deserialize)]
enum WithData {
    Newtype(u16),
    Tuple(u16, u16),
    Struct { a: u16 },
}

#[derive(Serialize)]
struct Skipping {
    #[serde(skip_serializing_if = "Option::is_none")]
    a: Option<u16>,
    b: u16,
}

#[test]
fn what_has_no_layout_fails() {
    assert_eq!(encode(&Some(1_u16)), Err(EncodeError::Unsupported));
    assert_eq!(encode(&None::<u16>), Err(EncodeError::Unsupported));
    assert_eq!(
        encode(&BTreeMap::from([(1_u16, 2_u16)])),
        Err(EncodeError::Unsupported)
    );
    assert_eq!(encode(&WithData::Newtype(1)), Err(EncodeError::Unsupported));
    assert_eq!(
        encode(&WithData::Tuple(1, 2)),
        Err(EncodeError::Unsupported)
    );
    assert_eq!(
        encode(&WithData::Struct { a: 1 }),
        Err(EncodeError::Unsupported)
    );
    assert_eq!(
        encode(&Skipping { a: None, b: 1 }),
        Err(EncodeError::Unsupported)
    );

    assert_eq!(
        from_bytes::<Option<u16>>(&hex!("0001")),
        Err(DecodeError::Unsupported)
    );
    assert_eq!(
        from_bytes::<BTreeMap<u16, u16>>(&hex!("0001 0002")),
        Err(DecodeError::Unsupported)
    );
    for index in [hex!("0000 0001"), hex!("0001 0001"), hex!("0002 0001")] {
        assert_eq!(
            from_bytes::<WithData>(&index),
            Err(DecodeError::Unsupported)
        );
    }
}

#[test]
fn from_bytes_takes_all_the_bytes() {
    assert_eq!(
        from_bytes::<u16>(&hex!("0001 0002")),
        Err(DecodeError::TrailingBytes)
    );
    assert_eq!(
        from_bytes::<u32>(&hex!("0001")),
        Err(DecodeError::UnexpectedEnd)
    );
    assert_eq!(from_bytes::<u16>(&[]), Err(DecodeError::UnexpectedEnd));
}

#[test]
fn a_failed_take_leaves_the_bytes_as_they_were() {
    let bytes = hex!("0001 0005");
    let mut vps = Deserializer::new(&bytes);
    assert_eq!(vps.take::<(u16, bool)>(), Err(DecodeError::BadBool));
    assert_eq!(vps.rest(), bytes);
    assert_eq!(vps.take::<(u16, u32)>(), Err(DecodeError::UnexpectedEnd));
    assert_eq!(vps.rest(), bytes);
    assert_eq!(vps.take::<u32>(), Ok(0x0001_0005));
}

#[test]
fn encodes_at_the_start_of_the_buffer() {
    let mut buf = [0xCC; 8];
    assert_eq!(
        &*to_slice(&0x1234_5678_u32, &mut buf).unwrap(),
        hex!("12345678")
    );
    assert_eq!(buf, hex!("12345678 CCCCCCCC"));
    assert_eq!(
        to_slice(&0x1234_5678_u32, &mut [0; 3]),
        Err(EncodeError::BufferFull)
    );
    assert_eq!(
        encoded_len(&MyData {
            a_msb: 0,
            a_lsb: 0,
            b: 0,
            c: 0
        }),
        Ok(8)
    );
}

/// Builds a write of `value` to `vp`, encoding it in place.
fn write<T: Serialize>(vp: u16, value: &T, crc: Option<Crc>) -> Vec<u8> {
    let mut buf = [0; MAX_FRAME];
    let mut writer = Request::begin_write(&mut buf, Address::Vp(vp), crc).unwrap();
    writer
        .fill(|room| to_slice(value, room).map(|data| data.len()))
        .unwrap();
    writer.finish().to_vec()
}

/// Decodes the data of a whole response.
fn answer<T: DeserializeOwned>(frame: &[u8], crc: Option<Crc>) -> T {
    let (Response::Data { data, .. }, []) = Response::decode(frame, crc).unwrap() else {
        panic!("not one data frame");
    };
    from_bytes(data).unwrap()
}

/// `PIC_Set` (`0x0084`): `5A`, the mode, then the page.
#[derive(Serialize)]
struct PicSet {
    enable: u8,
    mode: u8,
    page: Page,
}

/// `Music_Play_Set` (`0x00A0`), as the CFG file picks the WAE file.
#[derive(Serialize)]
struct MusicPlay {
    clip: u8,
    segments: u8,
    volume: u8,
    status: u8,
}

/// `RTC` (`0x0010`).
#[derive(Debug, PartialEq, Serialize, Deserialize)]
struct Rtc {
    year: u8,
    month: u8,
    day: u8,
    weekday: u8,
    hour: u8,
    minute: u8,
    second: u8,
    unused: u8,
}

/// `LED_Config` (`0x0082`): brightness on and in standby, then the time to
/// standby in 10 ms.
#[derive(Serialize)]
struct LedConfig {
    on: u8,
    standby: u8,
    standby_after: u16,
}

#[test]
fn speaks_the_guides_system_variable_examples() {
    let page_1 = PicSet {
        enable: 0x5A,
        mode: 0x01,
        page: Page(1),
    };
    assert_eq!(
        write(0x0084, &page_1, Crc::off()),
        hex!("5AA5 07 82 0084 5A01 0001")
    );

    let clip_3 = MusicPlay {
        clip: 3,
        segments: 1,
        volume: 0x40,
        status: 0,
    };
    assert_eq!(
        write(0x00A0, &clip_3, Crc::off()),
        hex!("5AA5 07 82 00A0 0301 4000")
    );

    let led = LedConfig {
        on: 0x64,
        standby: 0x32,
        standby_after: 1000,
    };
    assert_eq!(
        write(0x0082, &led, Crc::off()),
        hex!("5AA5 07 82 0082 6432 03E8")
    );

    // 2019-10-01 11:12:13, a Sunday by the guide.
    let rtc = Rtc {
        year: 0x13,
        month: 0x0A,
        day: 0x01,
        weekday: 0x00,
        hour: 0x0B,
        minute: 0x0C,
        second: 0x0D,
        unused: 0,
    };
    assert_eq!(
        write(0x0010, &rtc, Crc::off()),
        hex!("5AA5 0B 82 0010 130A 0100 0B0C 0D00")
    );
    assert_eq!(
        answer::<Rtc>(&hex!("5AA5 0C 83 0010 04 130A 0100 0B0C 0D00"), Crc::off()),
        rtc
    );
    assert_eq!(
        answer::<Page>(&hex!("5AA5 06 83 0014 01 0007"), Crc::off()),
        Page(7)
    );

    // UART2 configuration (`0x000C`): turning the CRC off, sent with one.
    assert_eq!(
        write(0x000C, &(0x5A_u8, 0x00_u8), Crc::on()),
        hex!("5AA5 07 82 000C 5A00 A6BD")
    );
}
