//! Key codes that basic touch controls ([`TouchAction::Basic`]) send.
//!
//! A data input's keyboard is made of basic touch controls: each sends its
//! key code to the number being typed.
//!
//! [`TouchAction::Basic`]: crate::TouchAction::Basic

/// No key: a basic touch that only switches page or shows its press effect.
pub const NONE: u16 = 0xFF00;
/// Decimal point.
pub const DOT: u16 = 0x002E;
/// Sign, `+` or `-`.
pub const SIGN: u16 = 0x002D;
/// Cancel: ends input without writing the VP.
pub const CANCEL: u16 = 0x00F0;
/// Enter: writes the number typed to the VP.
pub const ENTER: u16 = 0x00F1;
/// Backspace.
pub const BACKSPACE: u16 = 0x00F2;

/// The key code of digit `n`, 0-9.
///
/// # Panics
///
/// If `n` is over 9.
pub const fn digit(n: u8) -> u16 {
    assert!(n <= 9, "a digit is 0-9");
    0x0030 + n as u16
}
