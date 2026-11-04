use anyhow::{Result, anyhow, bail};
use std::io::{BufReader, Read};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Encoding {
    #[default]
    Utf8,
    Utf16Le,
    Utf16Be,
}

pub(super) struct Decoded<R> {
    reader: BufReader<R>,
    encoding: Encoding,
    started: bool,
    pending: [u8; 4],
    offset: usize,
    length: usize,
}

impl<R: Read> Decoded<R> {
    pub fn new(reader: R, encoding: Encoding) -> Self {
        Self {
            reader: BufReader::new(reader),
            encoding,
            started: false,
            pending: [0; 4],
            offset: 0,
            length: 0,
        }
    }

    fn raw_byte(&mut self) -> Result<Option<u8>> {
        let mut byte = [0];
        loop {
            match self.reader.read(&mut byte) {
                Ok(0) => return Ok(None),
                Ok(_) => return Ok(Some(byte[0])),
                Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
                Err(error) => return Err(error.into()),
            }
        }
    }

    fn unit(&mut self) -> Result<Option<u16>> {
        let Some(first) = self.raw_byte()? else {
            return Ok(None);
        };
        let second = self
            .raw_byte()?
            .ok_or_else(|| anyhow!("Incomplete UTF-16 byte pair"))?;
        Ok(Some(if self.encoding == Encoding::Utf16Le {
            u16::from_le_bytes([first, second])
        } else {
            u16::from_be_bytes([first, second])
        }))
    }

    fn scalar(&mut self) -> Result<Option<char>> {
        if self.encoding != Encoding::Utf8 {
            let Some(first) = self.unit()? else {
                return Ok(None);
            };
            let value = match first {
                0xd800..=0xdbff => {
                    let second = self
                        .unit()?
                        .ok_or_else(|| anyhow!("Incomplete UTF-16 surrogate pair"))?;
                    if !(0xdc00..=0xdfff).contains(&second) {
                        bail!("Invalid UTF-16 surrogate pair");
                    }
                    0x10000 + ((u32::from(first) - 0xd800) << 10) + u32::from(second) - 0xdc00
                }
                0xdc00..=0xdfff => bail!("Unpaired UTF-16 low surrogate"),
                _ => u32::from(first),
            };
            return Ok(Some(
                char::from_u32(value).ok_or_else(|| anyhow!("Invalid Unicode scalar"))?,
            ));
        }
        let Some(first) = self.raw_byte()? else {
            return Ok(None);
        };
        if first.is_ascii() {
            return Ok(Some(char::from(first)));
        }
        let length = match first {
            0xc2..=0xdf => 2,
            0xe0..=0xef => 3,
            0xf0..=0xf4 => 4,
            _ => bail!("Invalid UTF-8 leading byte"),
        };
        let mut bytes = [first, 0, 0, 0];
        for byte in &mut bytes[1..length] {
            *byte = self
                .raw_byte()?
                .ok_or_else(|| anyhow!("Incomplete UTF-8 sequence"))?;
        }
        Ok(std::str::from_utf8(&bytes[..length])?.chars().next())
    }

    pub fn next(&mut self) -> Result<Option<u8>> {
        if self.offset < self.length {
            let byte = self.pending[self.offset];
            self.offset += 1;
            return Ok(Some(byte));
        }
        loop {
            let Some(character) = self.scalar()? else {
                return Ok(None);
            };
            let first = !self.started;
            self.started = true;
            if first && character == '\u{feff}' {
                continue;
            }
            if first && self.encoding != Encoding::Utf8 && character == '\u{fffe}' {
                bail!("BOM does not match the selected encoding");
            }
            self.length = character.encode_utf8(&mut self.pending).len();
            self.offset = 1;
            return Ok(Some(self.pending[0]));
        }
    }
}
