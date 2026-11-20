/// The RGB565 colour the control files store for `0xRRGGBB`. Like the DGUS
/// editor, it drops the low bits of each channel.
///
/// ```
/// use degus_controls::rgb565;
///
/// assert_eq!(rgb565(0xFF8000), 0xFC00);
/// assert_eq!(rgb565(0xFFFFFF), 0xFFFF);
/// ```
pub const fn rgb565(rgb: u32) -> u16 {
    let (r, g, b) = ((rgb >> 16) & 0xFF, (rgb >> 8) & 0xFF, rgb & 0xFF);
    ((r >> 3) << 11 | (g >> 2) << 5 | b >> 3) as u16
}
