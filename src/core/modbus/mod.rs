use super::*;

#[cfg(feature = "rtu")]
pub mod rtu;

use byteorder::{BigEndian, LittleEndian, ReadBytesExt, WriteBytesExt};
use core::{convert::TryInto, fmt, mem, str};
use std::io::Read;
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DecodeError {
    InsufficientInput,
    InvalidInput,
    InvalidData,
}

impl fmt::Display for DecodeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        use DecodeError::*;
        match self {
            InsufficientInput => write!(f, "Insufficient input"),
            InvalidInput => write!(f, "Invalid input"),
            InvalidData => write!(f, "Invalid data"),
        }
    }
}

#[cfg(feature = "std")]
impl std::error::Error for DecodeError {}

pub type DecodeResult<T> = Result<T, DecodeError>;

fn decode_be_u16_from_bytes(input: &[u8]) -> DecodeResult<(u16, &[u8])> {
    if input.len() < mem::size_of::<u16>() {
        return Err(DecodeError::InsufficientInput);
    }
    let (head, rest) = input.split_at(mem::size_of::<u16>());
    if let Ok(bytes) = head.try_into() {
        Ok((u16::from_be_bytes(bytes), rest))
    } else {
        Err(DecodeError::InvalidInput)
    }
}

fn decode_be_u32_from_bytes(input: &[u8]) -> DecodeResult<(u32, &[u8])> {
    if input.len() < mem::size_of::<u32>() {
        return Err(DecodeError::InsufficientInput);
    }
    let (head, rest) = input.split_at(mem::size_of::<u32>());
    if let Ok(bytes) = head.try_into() {
        Ok((u32::from_be_bytes(bytes), rest))
    } else {
        Err(DecodeError::InvalidInput)
    }
}

pub const TEMPERATURE_REG_START: u16 = 0x017E; //d382
pub const TEMPERATURE_REG_COUNT: u16 = 0x0002;
/*
pub fn decode_temperature_from_u16(input: u16) -> DecodeResult<Temperature> {
    let degree_celsius = f64::from(i32::from(input) - 10000i32) / 100f64;
    Ok(Temperature::from_degree_celsius(degree_celsius))
}
pub fn decode_temperature_from_bytes(input: &[u8]) -> DecodeResult<(Temperature, &[u8])> {
    decode_be_u16_from_bytes(input).and_then(|(val, rest)| Ok((decode_temperature_from_u16(val)?, rest)))
}*/

//convert u16 words from xmttr to float
// TODO: extend this for all types of reads (anything > 16 ) this fn Byte Order 3-4-1-2
pub fn decode_f32_reg(read_bytes: Vec<u16>) -> DecodeResult<Temperature> {
    let msb_word: u16 = read_bytes[0];
    let first_byte: u8 = (msb_word >> 8) as u8;
    let second_byte: u8 = msb_word as u8;
    let lsb_word: u16 = read_bytes[1];
    let third_byte: u8 = (lsb_word >> 8) as u8;
    let fourth_byte: u8 = lsb_word as u8;
    //byte order is (3-4-1-2)
    let new_bytes: [u8; 4] = [third_byte, fourth_byte, first_byte, second_byte];
    //convert be_bytes to float
    let float_value: f32 = f32::from_be_bytes(new_bytes);
    Ok(Temperature::from_degree_celsius(float_value))
}

/// decode Generic register
pub fn decode_generic_reg(read_bytes: Vec<u16>) -> DecodeResult<Generic> {
    //setup the new u8 vec
    //go to len x2 and split string at '\0L'
    let mut vec_u8: Vec<u8> = Vec::with_capacity(read_bytes.len() * 2);
    read_bytes.into_iter().for_each(|val| {
        vec_u8.extend(&val.to_be_bytes());
    });
    // TODO: refactor, exception panics here
    let split_string = String::from_utf8(vec_u8).expect("invalid utf-8 seq");
    let s: Vec<&str> = split_string.split("\0L").collect();

    Ok(Generic::from_generic(s[0].to_string()))
}

pub const USER_MSG_REG_START: u16 = 0x67; //d103
pub const USER_MSG_REG_COUNT: u16 = 0x12; //Ascii len /2 i.e. A24 -> 12

pub const FW_REG_START: u16 = 0x04AF; //d1199
pub const FW_REG_COUNT: u16 = 0x0001;
//cast the bytes read from 'read input regs' to string

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct VolumetricWaterContentRaw(pub u16);

impl From<VolumetricWaterContentRaw> for VolumetricWaterContent {
    fn from(from: VolumetricWaterContentRaw) -> Self {
        let percent = f64::from(from.0) / 100f64;
        Self::from_percent(percent)
    }
}

pub const WATER_CONTENT_REG_START: u16 = 0x0001;
pub const WATER_CONTENT_REG_COUNT: u16 = 0x0001;

pub fn decode_water_content_from_u16(input: u16) -> DecodeResult<VolumetricWaterContent> {
    let percent = f64::from(input) / 100f64;
    let res = VolumetricWaterContent::from_percent(percent);
    if res.is_valid() {
        Ok(res)
    } else {
        Err(DecodeError::InvalidData)
    }
}

pub fn decode_water_content_from_bytes(
    input: &[u8],
) -> DecodeResult<(VolumetricWaterContent, &[u8])> {
    decode_be_u16_from_bytes(input)
        .and_then(|(val, rest)| Ok((decode_water_content_from_u16(val)?, rest)))
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RelativePermittivityRaw(pub u16);

impl From<RelativePermittivityRaw> for RelativePermittivity {
    fn from(from: RelativePermittivityRaw) -> Self {
        let ratio = f64::from(from.0) / 100f64;
        Self::from_ratio(ratio)
    }
}

pub const PERMITTIVITY_REG_START: u16 = 0x0002;
pub const PERMITTIVITY_REG_COUNT: u16 = 0x0001;

pub fn decode_permittivity_from_u16(input: u16) -> DecodeResult<RelativePermittivity> {
    let ratio = f64::from(input) / 100f64;
    let res = RelativePermittivity::from_ratio(ratio);
    if res.is_valid() {
        Ok(res)
    } else {
        Err(DecodeError::InvalidData)
    }
}

pub fn decode_permittivity_from_bytes(input: &[u8]) -> DecodeResult<(RelativePermittivity, &[u8])> {
    decode_be_u16_from_bytes(input)
        .and_then(|(val, rest)| Ok((decode_permittivity_from_u16(val)?, rest)))
}

pub const RAW_COUNTS_REG_START: u16 = 0x0003;
pub const RAW_COUNTS_REG_COUNT: u16 = 0x0001;

#[inline]
pub fn decode_raw_counts_from_u16(input: u16) -> DecodeResult<RawCounts> {
    Ok(input.into())
}

#[inline]
pub fn decode_raw_counts_from_bytes(input: &[u8]) -> DecodeResult<(RawCounts, &[u8])> {
    decode_be_u16_from_bytes(input)
        .and_then(|(val, rest)| Ok((decode_raw_counts_from_u16(val)?, rest)))
}

pub const BROADCAST_SLAVE_ADDR: u8 = 0x6F; //d111
pub const BROADCAST_REG_ADDR: u16 = 0x0138; //d312

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decode_temperature() {
        assert_eq!(
            Temperature::from_degree_celsius(-40.0),
            decode_temperature_from_bytes(&[0x17, 0x70]).unwrap().0
        );
        assert_eq!(
            Temperature::from_degree_celsius(0.0),
            decode_temperature_from_bytes(&[0x27, 0x10]).unwrap().0
        );
        assert_eq!(
            Temperature::from_degree_celsius(27.97),
            decode_temperature_from_bytes(&[0x31, 0xFD]).unwrap().0
        );
        assert_eq!(
            Temperature::from_degree_celsius(60.0),
            decode_temperature_from_bytes(&[0x3E, 0x80]).unwrap().0
        );
        assert_eq!(
            Temperature::from_degree_celsius(80.0),
            decode_temperature_from_bytes(&[0x46, 0x50]).unwrap().0
        );
    }

    #[test]
    fn decode_water_content() {
        // Valid range
        assert_eq!(
            VolumetricWaterContent::from_percent(0.0),
            decode_water_content_from_bytes(&[0x00, 0x00]).unwrap().0
        );
        assert_eq!(
            VolumetricWaterContent::from_percent(34.4),
            decode_water_content_from_bytes(&[0x0D, 0x70]).unwrap().0
        );
        assert_eq!(
            VolumetricWaterContent::from_percent(100.0),
            decode_water_content_from_bytes(&[0x27, 0x10]).unwrap().0
        );
        // Invalid range
        assert!(decode_water_content_from_bytes(&[0x27, 0x11]).is_err());
        assert!(decode_water_content_from_bytes(&[0xFF, 0xFF]).is_err());
    }

    #[test]
    fn decode_permittivity() {
        // Valid range
        assert_eq!(
            RelativePermittivity::from_ratio(1.0),
            decode_permittivity_from_bytes(&[0x00, 0x64]).unwrap().0
        );
        assert_eq!(
            RelativePermittivity::from_ratio(15.2),
            decode_permittivity_from_bytes(&[0x05, 0xF0]).unwrap().0
        );
        // Invalid range
        assert!(decode_permittivity_from_bytes(&[0x00, 0x00]).is_err());
        assert!(decode_permittivity_from_bytes(&[0x00, 0x63]).is_err());
    }
}
