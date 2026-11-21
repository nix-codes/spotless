//! MEGA's links and keys, as plain code: a public folder's link, a song's
//! place in it, and the keys its files are encrypted with.
//!
//! A folder's link carries its key. Each file's own key comes encrypted
//! with it (AES-128, block by block), and opens the file (AES-128 in
//! counter mode) and its attributes, such as its name (AES-128-CBC).

use aes::Aes128;
use aes::cipher::{BlockDecrypt, KeyInit};
use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;

/// A public folder, as its link names it.
// Read by the probes now, by importing a folder next.
#[cfg_attr(not(any(test, feature = "probes")), allow(dead_code))]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FolderLink {
    pub handle: String,
    /// In base 64, as in the link.
    pub key: String,
}

#[cfg_attr(not(any(test, feature = "probes")), allow(dead_code))]
impl FolderLink {
    /// Reads `https://mega.nz/folder/<handle>#<key>`, a subfolder's link
    /// after it included, and the older `https://mega.nz/#F!<handle>!<key>`.
    pub fn parse(text: &str) -> Option<Self> {
        let text = text.trim();
        let rest = ["https://mega.nz/", "https://mega.co.nz/", "mega.nz/"]
            .iter()
            .find_map(|start| text.strip_prefix(start))?;
        let (handle, key) = match rest.strip_prefix("folder/") {
            Some(rest) => rest.split_once('#')?,
            None => rest.strip_prefix("#F!")?.split_once('!')?,
        };
        // A subfolder's link names it after the key.
        let key = key.split(['/', '!']).next()?;
        let link = Self {
            handle: handle.to_string(),
            key: key.to_string(),
        };
        link.folder_key()?;
        Some(link).filter(|link| !link.handle.is_empty())
    }

    /// The folder's key: 16 bytes.
    pub fn folder_key(&self) -> Option<[u8; 16]> {
        folder_key(&self.key)
    }
}

/// A folder's key from its base 64.
pub fn folder_key(text: &str) -> Option<[u8; 16]> {
    decode(text)?.try_into().ok()
}

pub fn decode(text: &str) -> Option<Vec<u8>> {
    URL_SAFE_NO_PAD.decode(text.trim_end_matches('=')).ok()
}

/// A file's key, opened: what decrypts its contents and its attributes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FileKey {
    pub aes: [u8; 16],
    /// The counter's start: the file's nonce, then a count from 0.
    pub iv: [u8; 16],
}

impl FileKey {
    /// A file's key as a folder listing gives it (`<owner>:<base 64>`),
    /// encrypted with the folder's key.
    pub fn open(given: &str, folder_key: &[u8; 16]) -> Option<Self> {
        let encrypted = decode(given.rsplit(':').next()?)?;
        if encrypted.len() != 32 {
            return None;
        }
        let cipher = Aes128::new(folder_key.into());
        let mut key = [0u8; 32];
        for (block, out) in encrypted.chunks(16).zip(key.chunks_mut(16)) {
            let mut block = aes::Block::clone_from_slice(block);
            cipher.decrypt_block(&mut block);
            out.copy_from_slice(&block);
        }
        Some(Self::from_raw(&key))
    }

    /// The 32 bytes a file's key is: its AES key folded with its nonce and
    /// MAC, then the nonce.
    fn from_raw(key: &[u8; 32]) -> Self {
        let mut aes = [0u8; 16];
        for (i, byte) in aes.iter_mut().enumerate() {
            *byte = key[i] ^ key[i + 16];
        }
        let mut iv = [0u8; 16];
        iv[..8].copy_from_slice(&key[16..24]);
        Self { aes, iv }
    }

    /// A file's name, from its attributes (`MEGA{"n":…}`, encrypted).
    pub fn name(&self, attributes: &str) -> Option<String> {
        let mut data = decode(attributes)?;
        if data.is_empty() || !data.len().is_multiple_of(16) {
            return None;
        }
        let cipher = Aes128::new(&self.aes.into());
        let mut before = [0u8; 16];
        for block in data.chunks_mut(16) {
            let encrypted: [u8; 16] = block.try_into().ok()?;
            let mut plain = aes::Block::clone_from_slice(block);
            cipher.decrypt_block(&mut plain);
            for (out, (plain, before)) in block.iter_mut().zip(plain.iter().zip(before)) {
                *out = plain ^ before;
            }
            before = encrypted;
        }
        let text = std::str::from_utf8(&data).ok()?;
        let json = text.strip_prefix("MEGA")?.trim_end_matches('\0');
        let value: serde_json::Value = serde_json::from_str(json).ok()?;
        Some(value.get("n")?.as_str()?.to_string())
    }
}

#[cfg(test)]
mod tests {
    use aes::cipher::BlockEncrypt;

    use super::*;

    const FOLDER_KEY: [u8; 16] = *b"0123456789abcdef";

    fn encode(bytes: &[u8]) -> String {
        URL_SAFE_NO_PAD.encode(bytes)
    }

    /// A file's raw key, encrypted with the folder's key as a listing
    /// gives it.
    fn given(raw: &[u8; 32]) -> String {
        let cipher = Aes128::new(&FOLDER_KEY.into());
        let mut out = Vec::new();
        for block in raw.chunks(16) {
            let mut block = aes::Block::clone_from_slice(block);
            cipher.encrypt_block(&mut block);
            out.extend_from_slice(&block);
        }
        format!("owner:{}", encode(&out))
    }

    #[test]
    fn folder_links_old_and_new_are_read() {
        let key = encode(&FOLDER_KEY);
        let link = FolderLink {
            handle: "AbCd1234".into(),
            key: key.clone(),
        };
        for text in [
            format!("https://mega.nz/folder/AbCd1234#{key}"),
            format!("  https://mega.nz/folder/AbCd1234#{key}/folder/Sub12345 "),
            format!("https://mega.nz/#F!AbCd1234!{key}"),
            format!("mega.nz/folder/AbCd1234#{key}"),
        ] {
            assert_eq!(FolderLink::parse(&text).as_ref(), Some(&link), "{text}");
        }
        assert_eq!(link.folder_key(), Some(FOLDER_KEY));
        for text in [
            "https://mega.nz/folder/AbCd1234",
            "https://mega.nz/folder/AbCd1234#short",
            "https://mega.nz/file/AbCd1234#key",
            "https://example.com/folder/AbCd1234#key",
        ] {
            assert_eq!(FolderLink::parse(text), None, "{text}");
        }
    }

    #[test]
    fn a_files_key_opens_with_the_folders() {
        let mut raw = [0u8; 32];
        for (i, byte) in raw.iter_mut().enumerate() {
            *byte = i as u8;
        }
        let key = FileKey::open(&given(&raw), &FOLDER_KEY).unwrap();
        let aes: Vec<u8> = (0..16).map(|i| i ^ (i + 16)).collect();
        assert_eq!(key.aes.as_slice(), aes.as_slice());
        assert_eq!(key.iv, [16, 17, 18, 19, 20, 21, 22, 23, 0, 0, 0, 0, 0, 0, 0, 0]);
        assert_eq!(FileKey::open("owner:short", &FOLDER_KEY), None);
    }

    #[test]
    fn a_files_name_is_in_its_attributes() {
        let key = FileKey::from_raw(&[7; 32]);
        let mut plain = br#"MEGA{"n":"01 Song.ogg","c":"x"}"#.to_vec();
        plain.resize(plain.len().next_multiple_of(16), 0);
        // CBC with a zero IV.
        let cipher = Aes128::new(&key.aes.into());
        let mut before = [0u8; 16];
        for block in plain.chunks_mut(16) {
            for (byte, b) in block.iter_mut().zip(before) {
                *byte ^= b;
            }
            let mut encrypted = aes::Block::clone_from_slice(block);
            cipher.encrypt_block(&mut encrypted);
            block.copy_from_slice(&encrypted);
            before.copy_from_slice(block);
        }
        assert_eq!(key.name(&encode(&plain)).as_deref(), Some("01 Song.ogg"));
        assert_eq!(key.name("bad"), None);
    }
}
