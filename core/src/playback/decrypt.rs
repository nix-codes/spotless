//! Audio decrypted as it's read: AES-128 in counter mode, which any part
//! of a file can be decrypted from, so seeking works. The key and the
//! counter's start are the source's.

use std::io::{self, Read, Seek, SeekFrom};

use aes::cipher::{KeyIvInit, StreamCipher, StreamCipherSeek};

type Aes128Ctr = ctr::Ctr128BE<aes::Aes128>;

pub struct Decrypt<R> {
    /// None: the file isn't encrypted, and is read as it is.
    cipher: Option<Aes128Ctr>,
    reader: R,
}

impl<R> Decrypt<R> {
    pub fn new(key: Option<[u8; 16]>, iv: [u8; 16], reader: R) -> Self {
        Self {
            cipher: key.map(|key| Aes128Ctr::new(&key.into(), &iv.into())),
            reader,
        }
    }
}

impl<R: Read> Read for Decrypt<R> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let read = self.reader.read(buf)?;
        if let Some(cipher) = &mut self.cipher {
            cipher.apply_keystream(&mut buf[..read]);
        }
        Ok(read)
    }
}

impl<R: Seek> Seek for Decrypt<R> {
    fn seek(&mut self, position: SeekFrom) -> io::Result<u64> {
        let at = self.reader.seek(position)?;
        if let Some(cipher) = &mut self.cipher {
            cipher.seek(at);
        }
        Ok(at)
    }
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use super::*;

    #[test]
    fn decrypts_from_wherever_it_is_read() {
        let key = [7; 16];
        let iv = [3; 16];
        let plain: Vec<u8> = (0..1000u32).map(|i| (i % 251) as u8).collect();
        let mut encrypted = plain.clone();
        Aes128Ctr::new(&key.into(), &iv.into()).apply_keystream(&mut encrypted);
        let mut file = Decrypt::new(Some(key), iv, Cursor::new(encrypted));
        let mut all = Vec::new();
        file.read_to_end(&mut all).unwrap();
        assert_eq!(all, plain);
        file.seek(SeekFrom::Start(517)).unwrap();
        let mut some = [0; 10];
        file.read_exact(&mut some).unwrap();
        assert_eq!(some, plain[517..527]);
    }

    #[test]
    fn without_a_key_reads_the_file_as_it_is() {
        let mut file = Decrypt::new(None, [0; 16], Cursor::new(vec![1, 2, 3]));
        let mut all = Vec::new();
        file.read_to_end(&mut all).unwrap();
        assert_eq!(all, [1, 2, 3]);
    }
}
