//! On-disk constants of the `DGUS_2` WAE format. All integers are big-endian
//! except the CRC, which is stored low byte first.
//!
//! | Offset | Size        | Field                                            |
//! |--------|-------------|--------------------------------------------------|
//! | 0      | 6           | magic `DGUS_2`                                   |
//! | 6      | 2           | CRC-16/MODBUS of bytes `8..`, little-endian      |
//! | 8      | 4           | file length - 8                                  |
//! | 12     | 1           | version (3)                                      |
//! | 13     | 2           | number of clips                                  |
//! | 15     | 1           | sample format (0 = 16-bit PCM, 1 = 12:8)         |
//! | 16     | 8 * (max+1) | per clip ID: absolute offset, length; 0 = none   |

use crc::{CRC_16_MODBUS, Crc};

/// File magic.
pub const MAGIC: [u8; 6] = *b"DGUS_2";
/// Format version byte at offset 12.
pub const VERSION: u8 = 3;
/// Sample rate of every clip. The header does not record a rate, and the
/// T5L plays clips through a fixed 32 kHz PWM amplifier.
pub const SAMPLE_RATE: u32 = 32_000;
/// Size of the fixed file header that precedes the clip table.
pub(crate) const HEADER_LEN: usize = 16;
/// Size of one clip table entry: offset and length.
pub(crate) const ENTRY_LEN: usize = 8;
/// Clip data starts on, and is padded to, multiples of this.
pub(crate) const BLOCK_LEN: usize = 4096;
/// The CRC and length fields cover everything from this offset on.
pub(crate) const CRC_START: usize = 8;

const MODBUS: Crc<u16> = Crc::<u16>::new(&CRC_16_MODBUS);

pub(crate) fn checksum(data: &[u8]) -> u16 {
    MODBUS.checksum(data)
}

pub(crate) fn round_up_block(n: usize) -> usize {
    n.div_ceil(BLOCK_LEN) * BLOCK_LEN
}
