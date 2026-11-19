//! The examples in DWIN's T5L DGUSII Application Development Guide (§4.1,
//! system variable `0x000C`), both ends of the link together, and the edges
//! of the frame.

use degus_codec::{
    Accumulator, Address, Checksum, Crc, DecodeError, EncodeError, Feed, FrameWriter, MAX_CONTENT,
    MAX_FRAME, Request, Response, Space, split,
};
use hex_literal::hex;

fn encode_request(request: Request<'_>, crc: Option<Crc>) -> Vec<u8> {
    request.encode(&mut [0u8; MAX_FRAME], crc).unwrap().to_vec()
}

fn encode_response(response: Response<'_>, crc: Option<Crc>) -> Vec<u8> {
    response
        .encode(&mut [0u8; MAX_FRAME], crc)
        .unwrap()
        .to_vec()
}

/// Decodes one whole request, which must use up `frame`.
fn request(frame: &[u8], crc: Option<Crc>) -> Request<'_> {
    let (request, rest) = Request::decode(frame, crc).unwrap();
    assert_eq!(rest, []);
    request
}

/// Decodes one whole response, which must use up `frame`.
fn response(frame: &[u8], crc: Option<Crc>) -> Response<'_> {
    let (response, rest) = Response::decode(frame, crc).unwrap();
    assert_eq!(rest, []);
    response
}

/// Each guide example, as each end builds it and the other reads it.
#[test]
fn speaks_the_guides_examples() {
    let write = Request::Write {
        addr: Address::Vp(0x1000),
        data: &[0x31, 0x32],
    };
    let read = Request::Read {
        addr: Address::Vp(0x000F),
        len: 1,
    };
    let ack = Response::Ack(Space::Vp);
    let reply = Response::Data {
        addr: Address::Vp(0x000F),
        len: 1,
        data: &[0x14, 0x10],
    };
    let upload = Response::Data {
        addr: Address::Vp(0x1001),
        len: 1,
        data: &[0x00, 0x5A],
    };
    let examples: [(Option<Crc>, [&[u8]; 5]); 2] = [
        (
            Crc::off(),
            [
                &hex!("5AA5 05 82 1000 3132"),
                &hex!("5AA5 04 83 000F 01"),
                &hex!("5AA5 03 82 4F4B"),
                &hex!("5AA5 06 83 000F 01 1410"),
                &hex!("5AA5 06 83 1001 01 005A"),
            ],
        ),
        (
            Crc::on(),
            [
                &hex!("5AA5 07 82 1000 3132 CC9B"),
                &hex!("5AA5 06 83 000F 01 ED90"),
                &hex!("5AA5 05 82 4F4B A5EF"),
                &hex!("5AA5 08 83 000F 01 1410 43F0"),
                &hex!("5AA5 08 83 1001 01 005A 0E2C"),
            ],
        ),
    ];
    for (
        crc,
        [
            write_frame,
            read_frame,
            ack_frame,
            reply_frame,
            upload_frame,
        ],
    ) in examples
    {
        assert_eq!(encode_request(write, crc), write_frame);
        assert_eq!(request(write_frame, crc), write);
        assert_eq!(encode_request(read, crc), read_frame);
        assert_eq!(request(read_frame, crc), read);
        assert_eq!(encode_response(ack, crc), ack_frame);
        assert_eq!(response(ack_frame, crc), ack);
        assert_eq!(encode_response(reply, crc), reply_frame);
        assert_eq!(response(reply_frame, crc), reply);
        assert_eq!(encode_response(upload, crc), upload_frame);
        assert_eq!(response(upload_frame, crc), upload);
    }
}

/// Turning the CRC off over `0x000C`, then changing the baud rate as well,
/// both sent with the CRC on; then the version, read with it on.
#[test]
fn speaks_the_guides_uart2_examples() {
    let close = hex!("5AA5 07 82 000C 5A00 A6BD");
    let baud = hex!("5AA5 09 82 000C 5A00 001C BA18");
    assert_eq!(
        request(&close, Crc::on()),
        Request::Write {
            addr: Address::Vp(0x000C),
            data: &hex!("5A00"),
        }
    );
    assert_eq!(
        request(&baud, Crc::on()),
        Request::Write {
            addr: Address::Vp(0x000C),
            data: &hex!("5A00 001C"),
        }
    );
    let version = Response::Data {
        addr: Address::Vp(0x000F),
        len: 1,
        data: &hex!("6322"),
    };
    assert_eq!(
        encode_response(version, Crc::on()),
        hex!("5AA5 08 83 000F 01 6322 E5D5")
    );
}

/// The guide's register read: 4 bytes of R10 to R13 on page 0.
#[test]
fn reads_registers() {
    let read = Request::Read {
        addr: Address::Register {
            page: 0x00,
            reg: 0x0A,
        },
        len: 4,
    };
    assert_eq!(
        encode_request(read, Crc::off()),
        hex!("5AA5 04 81 00 0A 04")
    );
    assert_eq!(request(&hex!("5AA5 04 81 00 0A 04"), Crc::off()), read);
}

/// What one end builds, the other reads, in every address space.
#[test]
fn round_trips_every_space() {
    let addrs = [
        Address::Register {
            page: 0x08,
            reg: 0x10,
        },
        Address::Vp(0x1234),
        Address::Dword(0x0001_0203),
        Address::Curve(0x83),
    ];
    for crc in [Crc::off(), Crc::on()] {
        for addr in addrs {
            let write = Request::Write {
                addr,
                data: &hex!("BEEF"),
            };
            assert_eq!(request(&encode_request(write, crc), crc), write);
            let ack = Response::Ack(addr.space());
            assert_eq!(response(&encode_response(ack, crc), crc), ack);
        }

        // One unit of each readable space: a byte, a word, a double word.
        let units: [&[u8]; 3] = [&hex!("CA"), &hex!("CAFE"), &hex!("CAFEF00D")];
        for (addr, data) in addrs.into_iter().zip(units) {
            let read = Request::Read { addr, len: 1 };
            assert_eq!(request(&encode_request(read, crc), crc), read);
            let reply = Response::Data { addr, len: 1, data };
            assert_eq!(response(&encode_response(reply, crc), crc), reply);
        }
    }
}

#[test]
fn acks_carry_the_write_command() {
    assert_eq!(
        encode_response(Response::Ack(Space::Register), Crc::off()),
        hex!("5AA5 03 80 4F4B")
    );
    assert_eq!(
        encode_response(Response::Ack(Space::Dword), Crc::off()),
        hex!("5AA5 03 86 4F4B")
    );
    assert_eq!(
        Response::from_content(&hex!("82 4E4F")),
        Err(DecodeError::BadAck)
    );
    assert_eq!(
        Response::from_content(&hex!("82 4F")),
        Err(DecodeError::Truncated)
    );
}

#[test]
fn the_curve_buffer_is_write_only() {
    let read = Request::Read {
        addr: Address::Curve(0x01),
        len: 1,
    };
    assert_eq!(
        read.encode(&mut [0u8; MAX_FRAME], Crc::off()),
        Err(EncodeError::WriteOnly)
    );
    let data = Response::Data {
        addr: Address::Curve(0x01),
        len: 1,
        data: &hex!("0001"),
    };
    assert_eq!(
        data.encode(&mut [0u8; MAX_FRAME], Crc::off()).unwrap_err(),
        EncodeError::WriteOnly
    );
    assert_eq!(
        Request::from_content(&hex!("85 01 02")),
        Err(DecodeError::UnknownCommand)
    );
    assert_eq!(
        Response::from_content(&hex!("85 01 02 0001")),
        Err(DecodeError::UnknownCommand)
    );
}

#[test]
fn rejects_what_is_no_frame() {
    // No such command.
    assert_eq!(
        Request::from_content(&hex!("88 0000 01")),
        Err(DecodeError::UnknownCommand)
    );
    assert_eq!(
        Response::from_content(&hex!("7F 0000 01")),
        Err(DecodeError::UnknownCommand)
    );
    // A read without its length, a write without all of its address.
    assert_eq!(
        Request::from_content(&hex!("83 1000")),
        Err(DecodeError::Truncated)
    );
    assert_eq!(
        Request::from_content(&hex!("86 0001 02")),
        Err(DecodeError::Truncated)
    );
    assert_eq!(Request::from_content(&[]), Err(DecodeError::Truncated));
    // A bit flipped in the CRC.
    assert_eq!(
        Request::decode(&hex!("5AA5 06 83 000F 01 ED91"), Crc::on()).unwrap_err(),
        DecodeError::BadCrc
    );
}

#[test]
fn an_empty_write_decodes() {
    assert_eq!(
        Request::from_content(&hex!("82 1000")),
        Ok(Request::Write {
            addr: Address::Vp(0x1000),
            data: &[],
        })
    );
}

#[test]
fn bytes_after_a_reads_length_are_ignored() {
    assert_eq!(
        Request::from_content(&hex!("83 1000 02 FFFF")),
        Ok(Request::Read {
            addr: Address::Vp(0x1000),
            len: 2,
        })
    );
}

#[test]
fn split_takes_one_frame_and_returns_the_rest() {
    let input = hex!("5AA5 05 82 4F4B A5EF 0102");
    assert_eq!(
        split(&input, Crc::on()),
        Ok((&hex!("82 4F4B")[..], &hex!("0102")[..]))
    );
    // The CRC trailer counts towards the length byte.
    assert_eq!(
        split(&input, Crc::off()),
        Ok((&hex!("82 4F4B A5EF")[..], &hex!("0102")[..]))
    );
    assert_eq!(
        split(&hex!("5AA5 0A 83 12"), Crc::off()),
        Err(DecodeError::Truncated)
    );
    assert_eq!(
        split(&hex!("5AA5 02 82 4F"), Crc::off()),
        Err(DecodeError::BadLength)
    );
    // Too short once the CRC is counted.
    assert_eq!(
        split(&hex!("5AA5 04 83 000F 01"), Crc::on()),
        Err(DecodeError::BadLength)
    );
    assert_eq!(
        split(&hex!("A55A 03 82 4F4B"), Crc::off()),
        Err(DecodeError::BadHeader)
    );
    assert_eq!(
        split(&hex!("5AA5"), Crc::off()),
        Err(DecodeError::Truncated)
    );
}

/// A VP write's command and address take 3 bytes, a reply's 4 with its
/// length: so 126 words fill a write without a CRC and 125 with, 125 fill a
/// reply without and 124 with.
#[test]
fn frames_stay_within_the_length_byte() {
    let words = [0xAB; 2 * 127];
    for (crc, fits) in [(Crc::off(), 126), (Crc::on(), 125)] {
        let write = |n: usize| Request::Write {
            addr: Address::Vp(0x1000),
            data: &words[..2 * n],
        };
        let frame = encode_request(write(fits), crc);
        assert_eq!(frame.len(), MAX_FRAME);
        assert_eq!(usize::from(frame[2]), MAX_CONTENT);
        assert_eq!(
            write(fits + 1).encode(&mut [0u8; 400], crc),
            Err(EncodeError::FrameTooLong)
        );
    }
    for (crc, fits) in [(Crc::off(), 125), (Crc::on(), 124)] {
        let reply = |n: usize| Response::Data {
            addr: Address::Vp(0x1000),
            len: u8::try_from(n).unwrap(),
            data: &words[..2 * n],
        };
        // 4 + 2 * fits (+ 2) = 254, one byte short of the most.
        assert_eq!(encode_response(reply(fits), crc).len(), MAX_FRAME - 1);
        assert_eq!(
            reply(fits + 1).encode(&mut [0u8; 400], crc),
            Err(EncodeError::FrameTooLong)
        );
    }
}

#[test]
fn a_short_buffer_is_full() {
    let ack = Response::Ack(Space::Vp);
    // 6 bytes hold an ack without a CRC, not with one.
    assert_eq!(ack.encode(&mut [0u8; 6], Crc::off()).unwrap().len(), 6);
    assert_eq!(
        ack.encode(&mut [0u8; 7], Crc::on()),
        Err(EncodeError::BufferFull)
    );
    assert_eq!(ack.encode(&mut [0u8; 8], Crc::on()).unwrap().len(), 8);
    assert_eq!(
        FrameWriter::new(&mut [0u8; 2], Crc::off()).unwrap_err(),
        EncodeError::BufferFull
    );
    assert_eq!(
        FrameWriter::new(&mut [0u8; 4], Crc::on()).unwrap_err(),
        EncodeError::BufferFull
    );
}

#[test]
fn writes_data_in_place() {
    let mut buf = [0u8; MAX_FRAME];
    let mut writer = Response::begin_data(&mut buf, Address::Vp(0x000F), 2, Crc::on()).unwrap();
    writer
        .reserve(4)
        .unwrap()
        .copy_from_slice(&hex!("1410 6322"));
    assert_eq!(writer.content(), hex!("83 000F 02 1410 6322"));
    let frame = writer.finish();
    assert_eq!(
        response(frame, Crc::on()),
        Response::Data {
            addr: Address::Vp(0x000F),
            len: 2,
            data: &hex!("1410 6322"),
        }
    );
    // A failed reserve leaves the frame as it was.
    let mut writer = Request::begin_write(&mut buf, Address::Vp(0x1000), Crc::off()).unwrap();
    assert_eq!(
        writer.reserve(MAX_CONTENT).unwrap_err(),
        EncodeError::FrameTooLong
    );
    writer.push(&hex!("3132")).unwrap();
    assert_eq!(writer.finish(), hex!("5AA5 05 82 1000 3132"));
}

/// A checksum lent from elsewhere, as a CRC peripheral would be.
#[test]
fn takes_a_borrowed_checksum() {
    struct Counting(usize, Crc);
    impl Checksum for Counting {
        fn compute(&mut self, content: &[u8]) -> u16 {
            self.0 += 1;
            self.1.compute(content)
        }
    }

    let mut peripheral = Counting(0, Crc);
    let frame = Response::Ack(Space::Vp)
        .encode(&mut [0u8; 16], Some(&mut peripheral))
        .unwrap()
        .to_vec();
    assert_eq!(frame, hex!("5AA5 05 82 4F4B A5EF"));
    Response::decode(&frame, Some(&mut peripheral)).unwrap();
    assert_eq!(peripheral.0, 2);
}

/// Feeds `input` whole, collecting each frame's content or error.
fn feed_all<B: AsRef<[u8]> + AsMut<[u8]>>(
    acc: &mut Accumulator<B>,
    mut input: &[u8],
) -> Vec<Result<Vec<u8>, DecodeError>> {
    let mut out = Vec::new();
    loop {
        input = match acc.feed(input) {
            Feed::Consumed => return out,
            Feed::Frame(content, rest) => {
                out.push(Ok(content.to_vec()));
                rest
            }
            Feed::Error(error, rest) => {
                out.push(Err(error));
                rest
            }
        };
    }
}

#[test]
fn accumulates_frames_split_anywhere() {
    let bytes = hex!("5AA5 07 82 1000 3132 CC9B 5AA5 06 83 000F 01 ED90");
    let mut acc = Accumulator::new([0u8; MAX_CONTENT], Crc::on());
    let mut out = Vec::new();
    for byte in bytes {
        out.extend(feed_all(&mut acc, &[byte]));
    }
    assert_eq!(
        out,
        [
            Ok(hex!("82 1000 3132").to_vec()),
            Ok(hex!("83 000F 01").to_vec())
        ]
    );
}

#[test]
fn drops_noise_byte_by_byte() {
    let mut acc = Accumulator::new([0u8; MAX_CONTENT], Crc::off());
    let out = feed_all(&mut acc, &hex!("00 11 5AA5 04 83 000F 01 22"));
    assert_eq!(
        out,
        [
            Err(DecodeError::BadHeader),
            Err(DecodeError::BadHeader),
            Ok(hex!("83 000F 01").to_vec()),
            Err(DecodeError::BadHeader),
        ]
    );
}

/// A stray `5A` before a frame costs only itself.
#[test]
fn finds_a_header_after_a_stray_5a() {
    let mut acc = Accumulator::new([0u8; MAX_CONTENT], Crc::off());
    let out = feed_all(&mut acc, &hex!("5A 5AA5 04 83 000F 01"));
    assert_eq!(
        out,
        [Err(DecodeError::BadHeader), Ok(hex!("83 000F 01").to_vec())]
    );
    // And `5A` then anything else is two bytes of noise.
    let out = feed_all(&mut acc, &hex!("5A 00"));
    assert_eq!(out, [Err(DecodeError::BadHeader)]);
}

#[test]
fn drops_a_bad_frame_and_finds_the_next() {
    let mut acc = Accumulator::new([0u8; MAX_CONTENT], Crc::on());
    let out = feed_all(
        &mut acc,
        &hex!("5AA5 06 83 000F 01 ED91 5AA5 06 83 000F 01 ED90"),
    );
    assert_eq!(
        out,
        [Err(DecodeError::BadCrc), Ok(hex!("83 000F 01").to_vec())]
    );

    // A length too short for a frame with a CRC: what follows is noise.
    let out = feed_all(&mut acc, &hex!("5AA5 04 83 000F 01"));
    assert_eq!(out[0], Err(DecodeError::BadLength));
    assert!(out[1..].iter().all(|r| *r == Err(DecodeError::BadHeader)));
}

#[test]
fn refuses_frames_longer_than_its_buffer() {
    let mut buf = [0u8; 4];
    let mut acc = Accumulator::new(&mut buf[..], Crc::off());
    assert_eq!(
        feed_all(&mut acc, &hex!("5AA5 05"))[..],
        [Err(DecodeError::TooLarge)]
    );
    // Content and CRC that fit are taken.
    assert_eq!(
        feed_all(&mut acc, &hex!("5AA5 04 83 000F 01")),
        [Ok(hex!("83 000F 01").to_vec())]
    );
}

#[test]
fn reset_drops_a_partial_frame() {
    let mut acc = Accumulator::new([0u8; MAX_CONTENT], Crc::off());
    assert_eq!(feed_all(&mut acc, &hex!("5AA5 05 82 10")), []);
    acc.reset();
    let out = feed_all(&mut acc, &hex!("00 5AA5 04 83 000F 01"));
    assert_eq!(
        out,
        [Err(DecodeError::BadHeader), Ok(hex!("83 000F 01").to_vec())]
    );
}
