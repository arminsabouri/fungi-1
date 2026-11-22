//! Symmetric encryption of messages using a shared secret, such as one exported
//! by `fungi-hpke`.
//!
//! The cipher is XChaCha20Poly1305 with a random nonce, which keeps sealing
//! stateless. The nonce and tag are detached from the ciphertext, with no format
//! specified.

#![cfg_attr(coverage_nightly, feature(coverage_attribute))]

use chacha20poly1305::aead::Generate as _;
use chacha20poly1305::aead::array::typenum::Unsigned as _;
use chacha20poly1305::{AeadInOut as _, KeyInit as _, KeySizeUser, XChaCha20Poly1305};
use zeroize::ZeroizeOnDrop;

pub const KEY_BYTES: usize = <XChaCha20Poly1305 as KeySizeUser>::KeySize::USIZE;

#[derive(ZeroizeOnDrop)]
pub struct Key([u8; KEY_BYTES]);

impl From<[u8; KEY_BYTES]> for Key {
    fn from(bytes: [u8; KEY_BYTES]) -> Self {
        Self(bytes)
    }
}

pub type Nonce = chacha20poly1305::XNonce;
pub type Tag = chacha20poly1305::Tag;

/// Failure to open a message: it was modified or truncated, or was not sealed
/// with this key and `aad`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("message cannot be decrypted")]
pub struct Error;

/// Encrypt `message` in place with `key` and return the nonce and the tag.
/// [`open`] needs the same `aad`. Put the message's position, such as its
/// mailbox index, in `aad` to bind the message to it. Panics if `message` is
/// 2^38 - 64 bytes or longer, the XChaCha20Poly1305 limit.
pub fn seal(key: &Key, aad: &[u8], message: &mut [u8]) -> (Nonce, Tag) {
    let nonce = Nonce::generate();
    let tag = XChaCha20Poly1305::new((&key.0).into())
        .encrypt_inout_detached(&nonce, aad, message.into())
        .expect("message is shorter than the XChaCha20Poly1305 limit");
    (nonce, tag)
}

/// Decrypt `message` in place with `key`. On error the contents of `message`
/// are unspecified.
pub fn open(
    key: &Key,
    aad: &[u8],
    nonce: &Nonce,
    message: &mut [u8],
    tag: &Tag,
) -> Result<(), Error> {
    XChaCha20Poly1305::new((&key.0).into())
        .decrypt_inout_detached(nonce, aad, message.into(), tag)
        .map_err(|_| Error)
}

#[cfg(test)]
#[cfg_attr(coverage_nightly, coverage(off))]
mod tests {
    use super::*;

    const AAD: &[u8] = b"aad";

    fn key() -> Key {
        Key::from([1; KEY_BYTES])
    }

    #[test]
    fn messages_round_trip() {
        for message in [&b""[..], b"message", &[0xff; 10_000]] {
            let mut buffer = message.to_vec();
            let (nonce, tag) = seal(&key(), AAD, &mut buffer);
            if !message.is_empty() {
                assert_ne!(buffer, message);
            }
            open(&key(), AAD, &nonce, &mut buffer, &tag).unwrap();
            assert_eq!(buffer, message);
        }
    }

    #[test]
    fn wrong_key_or_aad_is_rejected() {
        let mut buffer = b"message".to_vec();
        let (nonce, tag) = seal(&key(), AAD, &mut buffer);
        assert_eq!(
            open(
                &Key::from([2; KEY_BYTES]),
                AAD,
                &nonce,
                &mut buffer.clone(),
                &tag
            ),
            Err(Error)
        );
        assert_eq!(
            open(&key(), b"other", &nonce, &mut buffer, &tag),
            Err(Error)
        );
    }

    #[test]
    fn modified_message_is_rejected() {
        let mut buffer = b"message".to_vec();
        let (nonce, tag) = seal(&key(), AAD, &mut buffer);
        buffer[0] ^= 1;
        assert_eq!(open(&key(), AAD, &nonce, &mut buffer, &tag), Err(Error));
    }

    #[test]
    fn modified_nonce_is_rejected() {
        let mut buffer = b"message".to_vec();
        let (mut nonce, tag) = seal(&key(), AAD, &mut buffer);
        nonce[0] ^= 1;
        assert_eq!(open(&key(), AAD, &nonce, &mut buffer, &tag), Err(Error));
    }

    #[test]
    fn modified_tag_is_rejected() {
        let mut buffer = b"message".to_vec();
        let (nonce, mut tag) = seal(&key(), AAD, &mut buffer);
        tag[0] ^= 1;
        assert_eq!(open(&key(), AAD, &nonce, &mut buffer, &tag), Err(Error));
    }

    #[test]
    fn each_message_has_its_own_nonce() {
        let (first, _) = seal(&key(), AAD, &mut b"message".to_vec());
        let (second, _) = seal(&key(), AAD, &mut b"message".to_vec());
        assert_ne!(first, second);
    }
}
