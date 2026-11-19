//! Groups on MLS (A2, D-0145): Songbird's OpenMLS provider for devices.
//!
//! - [`crypto`]: X-Wing, HPKE and the rest on aws-lc-rs.
//! - [`DeviceProvider`]: that crypto, aws-lc-rs randomness, and OpenMLS's in-memory storage,
//!   which the messenger persists into the `SQLCipher` store after every group operation
//!   ([`DeviceProvider::snapshot`], [`DeviceProvider::restore`]).

pub mod crypto;

use openmls_memory_storage::MemoryStorage;
use openmls_traits::OpenMlsProvider;

use crate::{Error, Result};

/// The OpenMLS provider of this device.
#[derive(Debug, Default)]
pub struct DeviceProvider {
    crypto: crypto::DeviceCrypto,
    rand: crypto::DeviceRand,
    storage: MemoryStorage,
}

impl OpenMlsProvider for DeviceProvider {
    type CryptoProvider = crypto::DeviceCrypto;
    type RandProvider = crypto::DeviceRand;
    type StorageProvider = MemoryStorage;

    fn storage(&self) -> &Self::StorageProvider {
        &self.storage
    }

    fn crypto(&self) -> &Self::CryptoProvider {
        &self.crypto
    }

    fn rand(&self) -> &Self::RandProvider {
        &self.rand
    }
}

impl DeviceProvider {
    /// Every key and value OpenMLS stored (group state, own leaf keys, KeyPackages' private
    /// halves), for the encrypted store.
    pub fn snapshot(&self) -> Result<zeroize::Zeroizing<Vec<u8>>> {
        let values = self.storage.values.read().map_err(|_| Error::Storage)?;
        let mut out = zeroize::Zeroizing::new(Vec::new());
        out.push(1);
        out.extend_from_slice(
            &u32::try_from(values.len())
                .map_err(|_| Error::Storage)?
                .to_be_bytes(),
        );
        for (k, v) in values.iter() {
            crate::crypto::put_bytes32(&mut out, k)?;
            crate::crypto::put_bytes32(&mut out, v)?;
        }
        Ok(out)
    }

    /// The provider with what [`Self::snapshot`] saved.
    pub fn restore(bytes: &[u8]) -> Result<Self> {
        let mut r = crate::crypto::Reader::new(bytes);
        if r.u8()? != 1 {
            return Err(Error::Storage);
        }
        let n = r.u32()?;
        let mut map = std::collections::HashMap::new();
        for _ in 0..n {
            let k = r.bytes32(1 << 20)?.to_vec();
            let v = r.bytes32(16 << 20)?.to_vec();
            map.insert(k, v);
        }
        r.finish()?;
        let storage = MemoryStorage::default();
        *storage.values.write().map_err(|_| Error::Storage)? = map;
        Ok(Self {
            storage,
            ..Self::default()
        })
    }
}
