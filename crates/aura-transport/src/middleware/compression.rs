//! Compression Middleware

use super::stack::TransportMiddleware;
use super::handler::{TransportHandler, TransportOperation, TransportResult};
use aura_protocol::effects::AuraEffects;
use aura_types::{MiddlewareContext, MiddlewareResult};
use std::collections::HashMap;

#[derive(Debug, Clone)]
pub struct CompressionConfig {
    pub algorithm: CompressionAlgorithm,
    pub level: u8, // 1-9, where 9 is highest compression
    pub min_size_bytes: usize, // Don't compress data smaller than this
}

impl Default for CompressionConfig {
    fn default() -> Self {
        Self {
            algorithm: CompressionAlgorithm::Gzip,
            level: 6,
            min_size_bytes: 1024, // 1KB
        }
    }
}

#[derive(Debug, Clone)]
pub enum CompressionAlgorithm {
    Gzip,
    Deflate,
    Brotli,
    Lz4,
}

impl CompressionAlgorithm {
    fn compress(&self, data: &[u8], level: u8) -> Result<Vec<u8>, String> {
        // Placeholder compression - in real implementation would use actual compression libraries
        match self {
            CompressionAlgorithm::Gzip => {
                // Simulate compression by adding a header and reducing size by ~30%
                let mut compressed = Vec::with_capacity(data.len() * 7 / 10 + 10);
                compressed.extend_from_slice(b"GZIP");
                compressed.extend_from_slice(&(data.len() as u32).to_be_bytes());
                compressed.extend_from_slice(&level.to_be_bytes());
                // Simulate compression ratio based on level
                let ratio = (10 - level as usize) * 10 + 30; // 30-120% size
                let target_size = data.len() * ratio / 100;
                compressed.resize(9 + target_size.min(data.len()), 0);
                Ok(compressed)
            }
            CompressionAlgorithm::Deflate => {
                let mut compressed = Vec::with_capacity(data.len() * 8 / 10 + 6);
                compressed.extend_from_slice(b"DEFL");
                compressed.extend_from_slice(&(data.len() as u32).to_be_bytes());
                let ratio = (10 - level as usize) * 8 + 40; // 40-112% size
                let target_size = data.len() * ratio / 100;
                compressed.resize(8 + target_size.min(data.len()), 0);
                Ok(compressed)
            }
            CompressionAlgorithm::Brotli => {
                let mut compressed = Vec::with_capacity(data.len() * 6 / 10 + 8);
                compressed.extend_from_slice(b"BROT");
                compressed.extend_from_slice(&(data.len() as u32).to_be_bytes());
                let ratio = (10 - level as usize) * 6 + 25; // 25-79% size (best compression)
                let target_size = data.len() * ratio / 100;
                compressed.resize(8 + target_size.min(data.len()), 0);
                Ok(compressed)
            }
            CompressionAlgorithm::Lz4 => {
                let mut compressed = Vec::with_capacity(data.len() * 9 / 10 + 4);
                compressed.extend_from_slice(b"LZ4\0");
                compressed.extend_from_slice(&(data.len() as u32).to_be_bytes());
                // LZ4 prioritizes speed over compression ratio
                let target_size = data.len() * 85 / 100; // ~85% size (fast)
                compressed.resize(8 + target_size.min(data.len()), 0);
                Ok(compressed)
            }
        }
    }
    
    fn decompress(&self, data: &[u8]) -> Result<Vec<u8>, String> {
        if data.len() < 8 {
            return Err("Invalid compressed data".to_string());
        }
        
        let header = &data[0..4];
        let expected_header = match self {
            CompressionAlgorithm::Gzip => b"GZIP",
            CompressionAlgorithm::Deflate => b"DEFL",
            CompressionAlgorithm::Brotli => b"BROT",
            CompressionAlgorithm::Lz4 => b"LZ4\0",
        };
        
        if header != expected_header {
            return Err(format!("Invalid compression header: expected {:?}, got {:?}", 
                              expected_header, header));
        }
        
        let original_size = u32::from_be_bytes([data[4], data[5], data[6], data[7]]) as usize;
        
        // Simulate decompression by creating data of original size
        Ok(vec![0; original_size])
    }
}

pub struct CompressionMiddleware {
    config: CompressionConfig,
    stats: CompressionStats,
}

#[derive(Debug, Default)]
struct CompressionStats {
    bytes_compressed: u64,
    bytes_decompressed: u64,
    compression_ratio: f64,
    operations: u64,
}

impl CompressionMiddleware {
    pub fn new() -> Self {
        Self {
            config: CompressionConfig::default(),
            stats: CompressionStats::default(),
        }
    }
    
    pub fn with_config(config: CompressionConfig) -> Self {
        Self {
            config,
            stats: CompressionStats::default(),
        }
    }
    
    fn should_compress(&self, data: &[u8]) -> bool {
        data.len() >= self.config.min_size_bytes
    }
    
    fn add_compression_metadata(&self, metadata: &mut HashMap<String, String>) {
        metadata.insert("compression".to_string(), format!("{:?}", self.config.algorithm));
        metadata.insert("compression_level".to_string(), self.config.level.to_string());
    }
    
    fn is_compressed(&self, metadata: &HashMap<String, String>) -> bool {
        metadata.contains_key("compression")
    }
}

impl Default for CompressionMiddleware {
    fn default() -> Self {
        Self::new()
    }
}

impl TransportMiddleware for CompressionMiddleware {
    fn process(
        &mut self,
        operation: TransportOperation,
        _context: &MiddlewareContext,
        effects: &dyn AuraEffects,
        next: &mut dyn TransportHandler,
    ) -> MiddlewareResult<TransportResult> {
        match operation {
            TransportOperation::Send { destination, data, mut metadata } => {
                let processed_data = if self.should_compress(&data) {
                    match self.config.algorithm.compress(&data, self.config.level) {
                        Ok(compressed) => {
                            self.add_compression_metadata(&mut metadata);
                            self.stats.bytes_compressed += data.len() as u64;
                            self.stats.operations += 1;
                            
                            let ratio = compressed.len() as f64 / data.len() as f64;
                            self.stats.compression_ratio = 
                                (self.stats.compression_ratio * (self.stats.operations - 1) as f64 + ratio) 
                                / self.stats.operations as f64;
                            
                            effects.log_info(
                                &format!("Compressed {} bytes to {} bytes (ratio: {:.2})", 
                                        data.len(), compressed.len(), ratio),
                                &[]
                            );
                            
                            compressed
                        }
                        Err(e) => {
                            effects.log_error(
                                &format!("Compression failed: {}", e),
                                &[]
                            );
                            data
                        }
                    }
                } else {
                    data
                };
                
                next.execute(TransportOperation::Send {
                    destination,
                    data: processed_data,
                    metadata,
                }, effects)
            }
            
            TransportOperation::Receive { source, timeout_ms } => {
                let result = next.execute(TransportOperation::Receive { source, timeout_ms }, effects)?;
                
                if let TransportResult::Received { source, data, metadata } = result {
                    let processed_data = if self.is_compressed(&metadata) {
                        if let Some(algorithm_str) = metadata.get("compression") {
                            let algorithm = match algorithm_str.as_str() {
                                "Gzip" => CompressionAlgorithm::Gzip,
                                "Deflate" => CompressionAlgorithm::Deflate,
                                "Brotli" => CompressionAlgorithm::Brotli,
                                "Lz4" => CompressionAlgorithm::Lz4,
                                _ => {
                                    effects.log_error(
                                        &format!("Unknown compression algorithm: {}", algorithm_str),
                                        &[]
                                    );
                                    return Ok(TransportResult::Received { source, data, metadata });
                                }
                            };
                            
                            match algorithm.decompress(&data) {
                                Ok(decompressed) => {
                                    self.stats.bytes_decompressed += decompressed.len() as u64;
                                    effects.log_info(
                                        &format!("Decompressed {} bytes to {} bytes", 
                                                data.len(), decompressed.len()),
                                        &[]
                                    );
                                    decompressed
                                }
                                Err(e) => {
                                    effects.log_error(
                                        &format!("Decompression failed: {}", e),
                                        &[]
                                    );
                                    data
                                }
                            }
                        } else {
                            data
                        }
                    } else {
                        data
                    };
                    
                    Ok(TransportResult::Received {
                        source,
                        data: processed_data,
                        metadata,
                    })
                } else {
                    Ok(result)
                }
            }
            
            _ => next.execute(operation, effects),
        }
    }
    
    fn middleware_name(&self) -> &'static str {
        "CompressionMiddleware"
    }
    
    fn middleware_info(&self) -> HashMap<String, String> {
        let mut info = HashMap::new();
        info.insert("algorithm".to_string(), format!("{:?}", self.config.algorithm));
        info.insert("level".to_string(), self.config.level.to_string());
        info.insert("min_size_bytes".to_string(), self.config.min_size_bytes.to_string());
        info.insert("bytes_compressed".to_string(), self.stats.bytes_compressed.to_string());
        info.insert("bytes_decompressed".to_string(), self.stats.bytes_decompressed.to_string());
        info.insert("avg_compression_ratio".to_string(), format!("{:.3}", self.stats.compression_ratio));
        info.insert("operations".to_string(), self.stats.operations.to_string());
        info
    }
}