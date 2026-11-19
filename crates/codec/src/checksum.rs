/// The CRC-16/MODBUS frames carry.
const MODBUS: crc::Crc<u16> = crc::Crc::<u16>::new(&crc::CRC_16_MODBUS);

/// Computes the CRC-16/MODBUS checksum of a frame's content.
///
/// [`Crc`] computes it in software. Implement it to use a CRC peripheral
/// instead.
pub trait Checksum {
    /// The checksum of `content`.
    fn compute(&mut self, content: &[u8]) -> u16;
}

impl<T: Checksum + ?Sized> Checksum for &mut T {
    fn compute(&mut self, content: &[u8]) -> u16 {
        (**self).compute(content)
    }
}

/// CRC-16/MODBUS in software.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub struct Crc;

impl Crc {
    /// Frames carry a CRC trailer.
    #[must_use]
    pub const fn on() -> Option<Self> {
        Some(Self)
    }

    /// Frames carry no checksum.
    #[must_use]
    pub const fn off() -> Option<Self> {
        None
    }
}

impl Checksum for Crc {
    fn compute(&mut self, content: &[u8]) -> u16 {
        MODBUS.checksum(content)
    }
}
