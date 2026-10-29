//! On-disk constants of the `DGUS_3` ICL format. All integers are big-endian
//! except the CRC, which is stored low byte first.
//!
//! | Offset | Size        | Field                                              |
//! |--------|-------------|----------------------------------------------------|
//! | 0      | 6           | magic `DGUS_3`                                     |
//! | 6      | 2           | CRC-16/MODBUS of bytes `8..`, little-endian        |
//! | 8      | 4           | file length - 8                                    |
//! | 12     | 1           | version (4)                                        |
//! | 13     | 2           | highest icon ID                                    |
//! | 15     | 1           | crypto flag (0 = none)                             |
//! | 16     | 4 * (max+1) | absolute record offset per icon ID, 0 = no icon    |

use crc::{CRC_16_MODBUS, Crc};

/// File magic.
pub const MAGIC: [u8; 6] = *b"DGUS_3";
/// Format version byte at offset 12.
pub const VERSION: u8 = 4;
/// Size of the fixed file header that precedes the offset table.
pub(crate) const HEADER_LEN: usize = 16;
/// Size of the per-icon record header: width, height, header length, stream length.
pub(crate) const RECORD_HEADER_LEN: usize = 10;
/// The CRC and length fields cover everything from this offset on.
pub(crate) const CRC_START: usize = 8;
/// An icon record must not straddle a multiple of this flash address.
pub(crate) const FLASH_BANK_SIZE: u64 = 16 * 1024 * 1024;

const MODBUS: Crc<u16> = Crc::<u16>::new(&CRC_16_MODBUS);

pub(crate) fn checksum(data: &[u8]) -> u16 {
    MODBUS.checksum(data)
}
