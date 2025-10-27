// Chunking strategy for large objects

use crate::manifest::{ChunkingParams, Cid};
use crate::storage::chunk_store::{ChunkId, ChunkMetadata};
use crate::Result;

/// Split data into chunks
pub fn chunk_data(data: &[u8], params: &ChunkingParams) -> Result<Vec<(ChunkId, Vec<u8>)>> {
    let chunk_size = params.chunk_size as usize;
    let mut chunks = Vec::new();

    // Temporary CID for chunk naming (would compute actual manifest CID first)
    let temp_hash = blake3::hash(data);
    let temp_cid = Cid::from_blake3_hash(&temp_hash);

    for (i, chunk_data) in data.chunks(chunk_size).enumerate() {
        let chunk_id = ChunkId::for_manifest_chunk(&temp_cid, i as u32);
        chunks.push((chunk_id, chunk_data.to_vec()));
    }

    Ok(chunks)
}

/// Reassemble chunks into original data
pub fn reassemble_chunks(chunks: &[(ChunkId, Vec<u8>)]) -> Result<Vec<u8>> {
    let total_size: usize = chunks.iter().map(|(_, data)| data.len()).sum();
    let mut result = Vec::with_capacity(total_size);

    for (_, chunk_data) in chunks {
        result.extend_from_slice(chunk_data);
    }

    Ok(result)
}

/// Compute chunk metadata
pub fn compute_chunk_metadata(
    chunk_id: &ChunkId,
    manifest_cid: &Cid,
    chunk_index: u32,
    data: &[u8],
    stored_at: u64,
) -> ChunkMetadata {
    ChunkMetadata {
        chunk_id: chunk_id.clone(),
        manifest_cid: manifest_cid.clone(),
        chunk_index,
        size: data.len() as u64,
        stored_at,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_chunking_roundtrip() {
        let data = vec![0u8; 5 * 1024 * 1024]; // 5 MiB
        let params = ChunkingParams::default_for_size(data.len() as u64);

        let chunks = chunk_data(&data, &params).unwrap();
        assert_eq!(chunks.len(), 5);

        let reassembled = reassemble_chunks(&chunks).unwrap();
        assert_eq!(reassembled, data);
    }
}
