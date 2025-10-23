// Chunking strategy for large objects

use crate::manifest::{ChunkId, ChunkMetadata, ChunkingParams};
use crate::Result;
use aura_journal::Cid;

/// Split data into chunks
pub fn chunk_data(data: &[u8], params: &ChunkingParams) -> Result<Vec<(ChunkId, Vec<u8>)>> {
    let chunk_size = params.chunk_size as usize;
    let mut chunks = Vec::new();
    
    // Temporary CID for chunk naming (would compute actual manifest CID first)
    let temp_cid = Cid::from_bytes(data);
    
    for (i, chunk_data) in data.chunks(chunk_size).enumerate() {
        let chunk_id = ChunkId::new(&temp_cid, i as u32);
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
    data: &[u8],
    offset: u64,
) -> ChunkMetadata {
    let cid = Cid::from_bytes(data);
    ChunkMetadata {
        chunk_id: chunk_id.clone(),
        size: data.len() as u64,
        cid,
        offset,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_chunking_roundtrip() {
        let data = vec![0u8; 5 * 1024 * 1024]; // 5 MiB
        let params = ChunkingParams::new(data.len() as u64);
        
        let chunks = chunk_data(&data, &params).unwrap();
        assert_eq!(chunks.len(), 5);
        
        let reassembled = reassemble_chunks(&chunks).unwrap();
        assert_eq!(reassembled, data);
    }
}

