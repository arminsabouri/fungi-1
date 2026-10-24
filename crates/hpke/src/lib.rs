//! One HPKE handshake that gives sender and recipient an exported shared
//! secret.
//!
//! The sender encrypts a message in place to the recipient's X-Wing public
//! key and sends the encapsulated key, the ChaCha20Poly1305 ciphertext and the
//! tag. The secret is exported from the HPKE context.
//!
//! The encapsulated key is not uniformly random. For example, its X25519
//! component always has the top bit clear, so anyone who sees payloads, such
//! as a mailbox server, can tell them apart from random bytes. Padding a
//! payload does not hide the encapsulated key. See
//! <https://github.com/fungi-protocol/fungi/issues/193>.

#![cfg_attr(coverage_nightly, feature(coverage_attribute))]

use hpke::aead::{AeadCtxR, AeadCtxS, AeadTag, ChaCha20Poly1305};
use hpke::kdf::HkdfSha256;
use hpke::kem::XWing;
use hpke::{HpkeError, OpModeR, OpModeS};
use zeroize::ZeroizeOnDrop;

/// Size of the exported shared secret.
pub const EXPORTED_SHARED_SECRET_BYTES: usize = 32;

/// HPKE info of the handshake. A domain separator, so it must be unique
/// to this protocol.
const INFO: &[u8] = b"fungi-hpke-info-v0";

/// HPKE exporter context of the exported shared secret. A domain separator,
/// so it must be unique to this use.
const EXPORTER_CONTEXT: &[u8] = b"fungi-hpke-exporter-context-v0";

type Kem = XWing;
/// X-Wing secret key that opens payloads.
pub type SecretKey = <Kem as hpke::Kem>::PrivateKey;
/// X-Wing public key that payloads are sealed to.
pub type PublicKey = <Kem as hpke::Kem>::PublicKey;
/// X-Wing encapsulated key of one handshake.
pub type EncappedKey = <Kem as hpke::Kem>::EncappedKey;
/// ChaCha20Poly1305 tag of the ciphertext.
pub type Tag = AeadTag<ChaCha20Poly1305>;

/// Secret exported from the handshake. Zeroized on drop.
#[derive(ZeroizeOnDrop)]
pub struct ExportedSharedSecret([u8; EXPORTED_SHARED_SECRET_BYTES]);

impl ExportedSharedSecret {
    /// The secret as bytes.
    pub fn as_bytes(&self) -> &[u8; EXPORTED_SHARED_SECRET_BYTES] {
        &self.0
    }

    /// Export the secret from either party's context. Consumes `context`, so no
    /// second secret can be exported from it under the same exporter context.
    fn from_context(context: impl Context) -> Self {
        let mut secret = Self([0; EXPORTED_SHARED_SECRET_BYTES]);
        context
            .export(EXPORTER_CONTEXT, &mut secret.0)
            .expect("EXPORTED_SHARED_SECRET_BYTES is a valid export length");
        secret
    }
}

/// HPKE context of either party, which the secret is exported from.
trait Context {
    fn export(&self, exporter_context: &[u8], out: &mut [u8]) -> Result<(), HpkeError>;
}

impl Context for AeadCtxS<ChaCha20Poly1305, HkdfSha256, Kem> {
    fn export(&self, exporter_context: &[u8], out: &mut [u8]) -> Result<(), HpkeError> {
        AeadCtxS::export(self, exporter_context, out)
    }
}

impl Context for AeadCtxR<ChaCha20Poly1305, HkdfSha256, Kem> {
    fn export(&self, exporter_context: &[u8], out: &mut [u8]) -> Result<(), HpkeError> {
        AeadCtxR::export(self, exporter_context, out)
    }
}

/// Failure to open a payload: it was modified or truncated, or was not sealed
/// to this key.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("payload cannot be decrypted")]
pub struct Error;

/// Encrypt `message` in place to `public_key` and return the encapsulated key,
/// the tag and the exported shared secret. Panics if `message` is 2^38 - 64
/// bytes or longer, the ChaCha20Poly1305 limit.
pub fn seal(
    public_key: &PublicKey,
    message: &mut [u8],
) -> (EncappedKey, Tag, ExportedSharedSecret) {
    let (encapsulated_key, mut context) =
        hpke::setup_sender::<ChaCha20Poly1305, HkdfSha256, Kem>(&OpModeS::Base, public_key, INFO)
            .expect("setting up to a valid public key cannot fail");
    let tag = context
        .seal_inout_detached(message.into(), &[])
        .expect("a fresh context seals a message shorter than the ChaCha20Poly1305 limit");
    let secret = ExportedSharedSecret::from_context(context);
    (encapsulated_key, tag, secret)
}

/// Decrypt `message` in place with `secret_key` and return the exported shared
/// secret. On error the contents of `message` are unspecified.
pub fn open(
    secret_key: &SecretKey,
    encapsulated_key: &EncappedKey,
    message: &mut [u8],
    tag: &Tag,
) -> Result<ExportedSharedSecret, Error> {
    let mut context = hpke::setup_receiver::<ChaCha20Poly1305, HkdfSha256, Kem>(
        &OpModeR::Base,
        secret_key,
        encapsulated_key,
        INFO,
    )
    .expect("X-Wing decapsulation cannot fail");
    context
        .open_inout_detached(message.into(), &[], tag)
        .map_err(|_| Error)?;
    Ok(ExportedSharedSecret::from_context(context))
}

#[cfg(test)]
#[cfg_attr(coverage_nightly, coverage(off))]
mod tests {
    use hpke::{Deserializable as _, Kem as _, Serializable as _};

    use super::*;

    fn keypair(seed: u8) -> (SecretKey, PublicKey) {
        Kem::derive_keypair(&[seed; 32])
    }

    #[test]
    fn messages_and_shared_secrets_round_trip() {
        let (secret, public) = keypair(1);
        for message in [&b""[..], b"message", &[0xff; 10_000]] {
            let mut buffer = message.to_vec();
            let (encapsulated_key, tag, sender_secret) = seal(&public, &mut buffer);
            if !message.is_empty() {
                assert_ne!(buffer, message);
            }
            let receiver_secret = open(&secret, &encapsulated_key, &mut buffer, &tag).unwrap();
            assert_eq!(buffer, message);
            assert_eq!(sender_secret.as_bytes(), receiver_secret.as_bytes());
        }
    }

    #[test]
    fn each_handshake_has_its_own_shared_secret() {
        let (_, public) = keypair(1);
        let (_, _, first) = seal(&public, &mut b"message".to_vec());
        let (_, _, second) = seal(&public, &mut b"message".to_vec());
        assert_ne!(first.as_bytes(), second.as_bytes());
    }

    #[test]
    fn modified_ciphertext_is_rejected() {
        let (secret, public) = keypair(1);
        let mut buffer = b"message".to_vec();
        let (encapsulated_key, tag, _) = seal(&public, &mut buffer);
        buffer[0] ^= 1;
        assert_eq!(
            open(&secret, &encapsulated_key, &mut buffer, &tag).err(),
            Some(Error)
        );
    }

    #[test]
    fn modified_tag_is_rejected() {
        let (secret, public) = keypair(1);
        let mut buffer = b"message".to_vec();
        let (encapsulated_key, tag, _) = seal(&public, &mut buffer);
        let mut tag = tag.to_bytes();
        tag[0] ^= 1;
        let tag = Tag::from_bytes(&tag).unwrap();
        assert_eq!(
            open(&secret, &encapsulated_key, &mut buffer, &tag).err(),
            Some(Error)
        );
    }

    #[test]
    fn modified_encapsulated_key_is_rejected() {
        let (secret, public) = keypair(1);
        let mut buffer = b"message".to_vec();
        let (encapsulated_key, tag, _) = seal(&public, &mut buffer);
        let mut encapsulated_key = encapsulated_key.to_bytes();
        encapsulated_key[0] ^= 1;
        let encapsulated_key = EncappedKey::from_bytes(&encapsulated_key).unwrap();
        assert_eq!(
            open(&secret, &encapsulated_key, &mut buffer, &tag).err(),
            Some(Error)
        );
    }

    #[test]
    fn wrong_key_is_rejected() {
        let (_, public) = keypair(1);
        let (other_secret, _) = keypair(2);
        let mut buffer = b"message".to_vec();
        let (encapsulated_key, tag, _) = seal(&public, &mut buffer);
        assert_eq!(
            open(&other_secret, &encapsulated_key, &mut buffer, &tag).err(),
            Some(Error)
        );
    }
}
