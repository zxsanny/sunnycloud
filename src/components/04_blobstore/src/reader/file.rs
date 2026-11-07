//! One blob file opened for reading: header, data key and the verification of single chunks.

use std::fs::File;
use std::io::ErrorKind;
use std::os::unix::fs::FileExt;
use std::path::Path;
use std::sync::Arc;

use contracts::BlobError;
use uuid::Uuid;

use super::probe::ReadProbe;
use crate::chunk::ChunkCipher;
use crate::error::{BlobIds, blob_error_from_io};
use crate::header::{HEADER_LEN, Header};
use crate::keyring::MasterKeyring;
use crate::layout::{BlobLayout, TAG_LEN};
use crate::path::blob_path;

pub(super) fn blob_ids(blob_id: Uuid) -> BlobIds {
    BlobIds {
        blob_id: Some(blob_id),
        account_id: None,
    }
}

pub(super) struct OpenedBlob {
    file: File,
    cipher: ChunkCipher,
    layout: BlobLayout,
    blob_id: Uuid,
    probe: Arc<ReadProbe>,
}

impl OpenedBlob {
    /// A missing file is `BlobMissing`; a header that is short or not what the writer wrote is `Corrupt`; a key id
    /// the ring lacks or a wrap that does not open is `KeyUnavailable`. The data key lives only as long as the
    /// cipher built from it.
    pub fn open(
        root: &Path,
        keyring: &MasterKeyring,
        probe: Arc<ReadProbe>,
        key_material: (&[u8], i16),
        blob_id: Uuid,
        layout: BlobLayout,
    ) -> Result<Self, BlobError> {
        let ids = blob_ids(blob_id);
        let file =
            File::open(blob_path(root, blob_id)).map_err(|error| blob_error_from_io(error, ids))?;
        let mut bytes = [0; HEADER_LEN];
        read_at(&file, &mut bytes, 0, ids)?;
        let header = Header::decode(&bytes)?;
        let (wrapped_key, master_key_id) = key_material;
        let key = keyring.unwrap_data_key(blob_id, wrapped_key, master_key_id)?;
        Ok(OpenedBlob {
            file,
            cipher: ChunkCipher::new(&key, header.nonce_prefix, blob_id),
            layout,
            blob_id,
            probe,
        })
    }

    /// The plaintext of chunk `index` once its tag verifies under this blob, index and last-chunk flag. A chunk that
    /// is cut short by the end of the file is `Corrupt`. The last chunk also needs the file to end exactly where the
    /// layout says, so appended bytes are `Corrupt` before any byte of that chunk is released.
    pub fn read_chunk(&self, index: u64) -> Result<Vec<u8>, BlobError> {
        let ids = blob_ids(self.blob_id);
        let offset = self
            .layout
            .chunk_offset(index)
            .ok_or(BlobError::OutOfRange)?;
        let plain_len = self.layout.plain_len(index).ok_or(BlobError::OutOfRange)?;
        if self.layout.is_last(index) {
            let length = self
                .file
                .metadata()
                .map_err(|error| blob_error_from_io(error, ids))?
                .len();
            if length != self.layout.file_len() {
                return Err(BlobError::Corrupt);
            }
        }
        let mut buffer =
            vec![0; usize::try_from(plain_len + TAG_LEN).map_err(|_| BlobError::OutOfRange)?];
        read_at(&self.file, &mut buffer, offset, ids)?;
        self.probe.chunk_read(buffer.len());
        self.cipher.open_chunk(&self.layout, index, &mut buffer)?;
        Ok(buffer)
    }
}

fn read_at(file: &File, buffer: &mut [u8], offset: u64, ids: BlobIds) -> Result<(), BlobError> {
    file.read_exact_at(buffer, offset).map_err(|error| {
        if error.kind() == ErrorKind::UnexpectedEof {
            BlobError::Corrupt
        } else {
            blob_error_from_io(error, ids)
        }
    })
}
