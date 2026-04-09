use std::io;

use serde::{Deserialize, Serialize};

/// Encrypted message wrapper for all network communication.
/// Contains the nonce and ciphertext produced by the Noise protocol.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EncryptedMessage {
    pub nonce: Vec<u8>,
    pub ciphertext: Vec<u8>,
}

/// Trait abstracting encryption/decryption operations.
/// Allows swapping Noise, TLS, or other backends without changing call sites.
pub trait Cipher: Send + Sync {
    /// Encrypt a plaintext buffer and return the ciphertext.
    fn encrypt(&mut self, plaintext: &[u8]) -> io::Result<Vec<u8>>;

    /// Decrypt a ciphertext buffer and return the plaintext.
    fn decrypt(&mut self, ciphertext: &[u8]) -> io::Result<Vec<u8>>;

    /// Return whether the cipher is fully initialized (handshake complete).
    fn is_ready(&self) -> bool;
}

/// Noise protocol implementation using the snow crate directly.
///
/// Design decisions:
/// - Uses snow library for Noise_XX_25519_ChaChaPoly_BLAKE2s pattern.
/// - XX pattern provides mutual authentication and key exchange.
/// - Handshake must be completed before any data transfer.
/// - After handshake, both sides hold symmetric cipher state.
pub struct NoiseCipher {
    /// The underlying snow transport state (after handshake).
    state: Option<snow::TransportState>,
    /// Whether this side initiated the handshake (true = initiator, false = responder).
    is_initiator: bool,
    /// Handshake state, present until handshake completes.
    handshake: Option<snow::HandshakeState>,
}

impl NoiseCipher {
    fn builder() -> snow::Builder<'static> {
        let pattern: snow::params::NoiseParams = "Noise_XX_25519_ChaChaPoly_BLAKE2s"
            .parse()
            .expect("valid noise pattern");
        snow::Builder::new(pattern)
    }

    /// Create a new Noise cipher as the initiator (client connecting to server).
    ///
    /// Uses the Noise_XX_25519_ChaChaPoly_BLAKE2s pattern.
    /// Both local and remote static public keys are required for mutual auth.
    pub fn new_initiator(
        local_static_key: &'static [u8; 32],
        remote_static_key: Option<&'static [u8; 32]>,
    ) -> io::Result<Self> {
        let mut builder = Self::builder()
            .local_private_key(local_static_key);

        if let Some(remote_key) = remote_static_key {
            builder = builder.remote_public_key(remote_key);
        }

        let handshake = builder
            .build_initiator()
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, format!("failed to build noise handshake: {e}")))?;

        Ok(Self {
            state: None,
            is_initiator: true,
            handshake: Some(handshake),
        })
    }

    /// Create a new Noise cipher as the responder (server accepting connection).
    pub fn new_responder(
        local_static_key: &'static [u8; 32],
        remote_static_key: Option<&'static [u8; 32]>,
    ) -> io::Result<Self> {
        let mut builder = Self::builder()
            .local_private_key(local_static_key);

        if let Some(remote_key) = remote_static_key {
            builder = builder.remote_public_key(remote_key);
        }

        let handshake = builder
            .build_responder()
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, format!("failed to build noise handshake: {e}")))?;

        Ok(Self {
            state: None,
            is_initiator: false,
            handshake: Some(handshake),
        })
    }

    /// Perform one step of the Noise handshake.
    ///
    /// If `input` is Some, it contains data from the peer.
    /// Returns the data to send to the peer.
    /// When the handshake completes, returns Ok(None).
    pub fn handshake_step(&mut self, input: Option<&[u8]>) -> io::Result<Option<Vec<u8>>> {
        let mut handshake = self
            .handshake
            .take()
            .ok_or_else(|| io::Error::new(io::ErrorKind::AlreadyExists, "handshake already complete"))?;

        let input = input.unwrap_or(&[]);
        let mut output = vec![0u8; input.len() + 16];
        let n = handshake
            .write_message(input, &mut output)
            .map_err(|e| io::Error::new(io::ErrorKind::ConnectionAborted, format!("handshake step failed: {e}")))?;

        output.truncate(n);

        if handshake.is_handshake_finished() {
            match handshake.into_transport_mode() {
                Ok(state) => {
                    self.state = Some(state);
                    self.handshake = None;
                }
                Err(e) => {
                    return Err(io::Error::new(
                        io::ErrorKind::ConnectionAborted,
                        format!("failed to finish handshake: {e}"),
                    ));
                }
            }
        } else {
            self.handshake = Some(handshake);
        }

        if output.is_empty() {
            Ok(None)
        } else {
            Ok(Some(output))
        }
    }

    /// Generate a new static keypair for use with this cipher.
    /// Returns (private_key, public_key).
    pub fn generate_keypair() -> io::Result<(Vec<u8>, Vec<u8>)> {
        let builder = Self::builder();
        let keypair = builder
            .generate_keypair()
            .map_err(|e| io::Error::new(io::ErrorKind::Other, format!("failed to generate keypair: {e}")))?;

        Ok((keypair.private, keypair.public))
    }
}

impl Cipher for NoiseCipher {
    fn encrypt(&mut self, plaintext: &[u8]) -> io::Result<Vec<u8>> {
        let state = self
            .state
            .as_mut()
            .ok_or_else(|| io::Error::new(io::ErrorKind::NotConnected, "cipher not ready, handshake incomplete"))?;

        let mut ciphertext = vec![0u8; plaintext.len() + 16];
        let len = state
            .write_message(plaintext, &mut ciphertext)
            .map_err(|e| io::Error::new(io::ErrorKind::ConnectionAborted, format!("encryption failed: {e}")))?;

        ciphertext.truncate(len);
        Ok(ciphertext)
    }

    fn decrypt(&mut self, ciphertext: &[u8]) -> io::Result<Vec<u8>> {
        let state = self
            .state
            .as_mut()
            .ok_or_else(|| io::Error::new(io::ErrorKind::NotConnected, "cipher not ready, handshake incomplete"))?;

        let mut plaintext = vec![0u8; ciphertext.len().saturating_sub(16)];
        let len = state
            .read_message(ciphertext, &mut plaintext)
            .map_err(|e| io::Error::new(io::ErrorKind::ConnectionAborted, format!("decryption failed: {e}")))?;

        plaintext.truncate(len);
        Ok(plaintext)
    }

    fn is_ready(&self) -> bool {
        self.state.is_some()
    }
}

/// A no-op cipher for testing or fallback when encryption is not available.
/// WARNING: This provides NO security and should only be used for debugging.
pub struct PlaintextCipher;

impl Cipher for PlaintextCipher {
    fn encrypt(&mut self, plaintext: &[u8]) -> io::Result<Vec<u8>> {
        Ok(plaintext.to_vec())
    }

    fn decrypt(&mut self, ciphertext: &[u8]) -> io::Result<Vec<u8>> {
        Ok(ciphertext.to_vec())
    }

    fn is_ready(&self) -> bool {
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_plaintext_cipher() {
        let mut cipher = PlaintextCipher;
        let data = b"hello world";
        let encrypted = cipher.encrypt(data).unwrap();
        assert_eq!(&encrypted, data);
        let decrypted = cipher.decrypt(&encrypted).unwrap();
        assert_eq!(&decrypted, data);
        assert!(cipher.is_ready());
    }

    #[test]
    fn test_noise_keypair_generation() {
        let (priv_key, pub_key) = NoiseCipher::generate_keypair().unwrap();
        assert_eq!(priv_key.len(), 32);
        assert_eq!(pub_key.len(), 32);
    }
}
