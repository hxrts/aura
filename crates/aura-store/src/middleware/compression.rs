//! Compression Middleware
//!
//! Provides transparent compression/decompression of stored data.

use super::handler::{StorageError, StorageHandler, StorageOperation, StorageResult};
use super::stack::StorageMiddleware;
use aura_protocol::effects::AuraEffects;
use aura_types::{AuraError, MiddlewareContext, MiddlewareResult};
use std::collections::HashMap;

/// Compression algorithms supported by the middleware
#[derive(Debug, Clone)]
pub enum CompressionAlgorithm {
    None,
    Gzip,
    Zstd,
    Lz4,
}

/// Configuration for compression middleware
#[derive(Debug, Clone)]
pub struct CompressionConfig {
    pub algorithm: CompressionAlgorithm,
    pub compression_level: u32,
    pub min_size_threshold: usize, // Don't compress files smaller than this
    pub max_compression_ratio: f32, // Skip compression if ratio is worse than this
}

impl Default for CompressionConfig {
    fn default() -> Self {
        Self {
            algorithm: CompressionAlgorithm::Zstd,
            compression_level: 3,       // Balanced compression level
            min_size_threshold: 1024,   // 1KB threshold
            max_compression_ratio: 0.9, // Only compress if we get at least 10% reduction
        }
    }
}

/// Compression middleware that compresses data before storage and decompresses on retrieval
pub struct CompressionMiddleware {
    config: CompressionConfig,
    stats: CompressionStats,
}

/// Statistics for compression operations
#[derive(Debug, Clone, Default)]
pub struct CompressionStats {
    pub total_compressions: u64,
    pub total_decompressions: u64,
    pub bytes_compressed: u64,
    pub bytes_decompressed: u64,
    pub compression_time_ms: u64,
    pub decompression_time_ms: u64,
}

impl CompressionMiddleware {
    /// Create new compression middleware with default configuration
    pub fn new() -> Self {
        Self {
            config: CompressionConfig::default(),
            stats: CompressionStats::default(),
        }
    }

    /// Create new compression middleware with custom configuration
    pub fn with_config(config: CompressionConfig) -> Self {
        Self {
            config,
            stats: CompressionStats::default(),
        }
    }

    /// Compress data using the configured algorithm
    fn compress_data(&mut self, data: &[u8]) -> Result<Option<Vec<u8>>, StorageError> {
        // Skip compression for small files
        if data.len() < self.config.min_size_threshold {
            return Ok(None);
        }

        let start_time = std::time::Instant::now();

        let compressed = match self.config.algorithm {
            CompressionAlgorithm::None => return Ok(None),

            CompressionAlgorithm::Gzip => {
                // Placeholder GZIP compression
                // In production, use flate2 or similar
                self.simple_compress(data)
            }

            CompressionAlgorithm::Zstd => {
                // Placeholder Zstd compression
                // In production, use zstd crate
                self.simple_compress(data)
            }

            CompressionAlgorithm::Lz4 => {
                // Placeholder LZ4 compression
                // In production, use lz4 crate
                self.simple_compress(data)
            }
        };

        // Check compression ratio
        let compression_ratio = compressed.len() as f32 / data.len() as f32;
        if compression_ratio > self.config.max_compression_ratio {
            // Compression didn't help much, skip it
            return Ok(None);
        }

        // Update statistics
        let elapsed = start_time.elapsed();
        self.stats.total_compressions += 1;
        self.stats.bytes_compressed += data.len() as u64;
        self.stats.compression_time_ms += elapsed.as_millis() as u64;

        Ok(Some(compressed))
    }

    /// Decompress data using the configured algorithm
    fn decompress_data(
        &mut self,
        compressed_data: &[u8],
        algorithm: &CompressionAlgorithm,
    ) -> Result<Vec<u8>, StorageError> {
        let start_time = std::time::Instant::now();

        let decompressed = match algorithm {
            CompressionAlgorithm::None => {
                return Err(StorageError::CompressionError {
                    message: "Attempted to decompress uncompressed data".to_string(),
                });
            }

            CompressionAlgorithm::Gzip => {
                // Placeholder GZIP decompression
                self.simple_decompress(compressed_data)
            }

            CompressionAlgorithm::Zstd => {
                // Placeholder Zstd decompression
                self.simple_decompress(compressed_data)
            }

            CompressionAlgorithm::Lz4 => {
                // Placeholder LZ4 decompression
                self.simple_decompress(compressed_data)
            }
        }?;

        // Update statistics
        let elapsed = start_time.elapsed();
        self.stats.total_decompressions += 1;
        self.stats.bytes_decompressed += decompressed.len() as u64;
        self.stats.decompression_time_ms += elapsed.as_millis() as u64;

        Ok(decompressed)
    }

    /// Simple placeholder compression (NOT a real compression algorithm)
    fn simple_compress(&self, data: &[u8]) -> Vec<u8> {
        // This is just a placeholder that removes duplicate consecutive bytes
        // In production, use proper compression libraries
        let mut compressed = Vec::new();
        let mut i = 0;

        while i < data.len() {
            let current_byte = data[i];
            let mut count = 1;

            // Count consecutive identical bytes
            while i + count < data.len() && data[i + count] == current_byte && count < 255 {
                count += 1;
            }

            if count > 3 {
                // Use run-length encoding for sequences of 4 or more
                compressed.push(0xFF); // Escape byte
                compressed.push(count as u8);
                compressed.push(current_byte);
            } else {
                // Just copy the bytes
                for _ in 0..count {
                    compressed.push(current_byte);
                }
            }

            i += count;
        }

        compressed
    }

    /// Simple placeholder decompression
    fn simple_decompress(&self, compressed_data: &[u8]) -> Result<Vec<u8>, StorageError> {
        let mut decompressed = Vec::new();
        let mut i = 0;

        while i < compressed_data.len() {
            if compressed_data[i] == 0xFF && i + 2 < compressed_data.len() {
                // Run-length encoded sequence
                let count = compressed_data[i + 1] as usize;
                let byte_value = compressed_data[i + 2];

                for _ in 0..count {
                    decompressed.push(byte_value);
                }

                i += 3;
            } else {
                // Regular byte
                decompressed.push(compressed_data[i]);
                i += 1;
            }
        }

        Ok(decompressed)
    }

    /// Get compression statistics
    pub fn get_stats(&self) -> &CompressionStats {
        &self.stats
    }

    /// Reset compression statistics
    pub fn reset_stats(&mut self) {
        self.stats = CompressionStats::default();
    }
}

impl Default for CompressionMiddleware {
    fn default() -> Self {
        Self::new()
    }
}

impl StorageMiddleware for CompressionMiddleware {
    fn process(
        &mut self,
        operation: StorageOperation,
        _context: &MiddlewareContext,
        effects: &dyn AuraEffects,
        next: &mut dyn StorageHandler,
    ) -> MiddlewareResult<StorageResult> {
        match operation {
            StorageOperation::Store {
                chunk_id,
                data,
                mut metadata,
            } => {
                // Attempt to compress the data
                match self
                    .compress_data(&data)
                    .map_err(|e| AuraError::internal_error(e.to_string()))?
                {
                    Some(compressed_data) => {
                        // Compression was beneficial, store compressed data
                        metadata.insert("compressed".to_string(), "true".to_string());
                        metadata.insert(
                            "compression_algorithm".to_string(),
                            format!("{:?}", self.config.algorithm),
                        );
                        metadata.insert("original_size".to_string(), data.len().to_string());
                        metadata.insert(
                            "compressed_size".to_string(),
                            compressed_data.len().to_string(),
                        );

                        let compressed_operation = StorageOperation::Store {
                            chunk_id,
                            data: compressed_data,
                            metadata,
                        };

                        next.execute(compressed_operation, effects)
                    }

                    None => {
                        // Compression was not beneficial, store original data
                        metadata.insert("compressed".to_string(), "false".to_string());
                        let original_operation = StorageOperation::Store {
                            chunk_id,
                            data,
                            metadata,
                        };
                        next.execute(original_operation, effects)
                    }
                }
            }

            StorageOperation::Retrieve { chunk_id: _ } => {
                // Retrieve the data first
                let result = next.execute(operation, effects)?;

                match result {
                    StorageResult::Retrieved {
                        chunk_id: retrieved_chunk_id,
                        data,
                        metadata,
                    } => {
                        // Check if data is compressed
                        if metadata
                            .get("compressed")
                            .map(|v| v == "true")
                            .unwrap_or(false)
                        {
                            // Parse compression algorithm from metadata
                            let algorithm = metadata
                                .get("compression_algorithm")
                                .map(|s| match s.as_str() {
                                    "Gzip" => CompressionAlgorithm::Gzip,
                                    "Zstd" => CompressionAlgorithm::Zstd,
                                    "Lz4" => CompressionAlgorithm::Lz4,
                                    _ => CompressionAlgorithm::Zstd, // Default fallback
                                })
                                .unwrap_or(CompressionAlgorithm::Zstd);

                            // Decompress the data
                            let decompressed_data = self
                                .decompress_data(&data, &algorithm)
                                .map_err(|e| AuraError::internal_error(e.to_string()))?;

                            Ok(StorageResult::Retrieved {
                                chunk_id: retrieved_chunk_id,
                                data: decompressed_data,
                                metadata,
                            })
                        } else {
                            // Data is not compressed, return as-is
                            Ok(StorageResult::Retrieved {
                                chunk_id: retrieved_chunk_id,
                                data,
                                metadata,
                            })
                        }
                    }
                    _ => Ok(result),
                }
            }

            // For other operations, pass through unchanged
            _ => next.execute(operation, effects),
        }
    }

    fn middleware_name(&self) -> &'static str {
        "CompressionMiddleware"
    }

    fn middleware_info(&self) -> HashMap<String, String> {
        let mut info = HashMap::new();
        info.insert(
            "algorithm".to_string(),
            format!("{:?}", self.config.algorithm),
        );
        info.insert(
            "compression_level".to_string(),
            self.config.compression_level.to_string(),
        );
        info.insert(
            "min_size_threshold".to_string(),
            self.config.min_size_threshold.to_string(),
        );
        info.insert(
            "max_compression_ratio".to_string(),
            self.config.max_compression_ratio.to_string(),
        );
        info.insert(
            "total_compressions".to_string(),
            self.stats.total_compressions.to_string(),
        );
        info.insert(
            "total_decompressions".to_string(),
            self.stats.total_decompressions.to_string(),
        );
        info.insert(
            "bytes_compressed".to_string(),
            self.stats.bytes_compressed.to_string(),
        );
        info.insert(
            "bytes_decompressed".to_string(),
            self.stats.bytes_decompressed.to_string(),
        );
        info
    }
}

/// Builder for compression middleware
pub struct CompressionBuilder {
    config: CompressionConfig,
}

impl CompressionBuilder {
    pub fn new() -> Self {
        Self {
            config: CompressionConfig::default(),
        }
    }

    pub fn with_algorithm(mut self, algorithm: CompressionAlgorithm) -> Self {
        self.config.algorithm = algorithm;
        self
    }

    pub fn with_compression_level(mut self, level: u32) -> Self {
        self.config.compression_level = level;
        self
    }

    pub fn with_min_size_threshold(mut self, threshold: usize) -> Self {
        self.config.min_size_threshold = threshold;
        self
    }

    pub fn with_max_compression_ratio(mut self, ratio: f32) -> Self {
        self.config.max_compression_ratio = ratio;
        self
    }

    pub fn build(self) -> CompressionMiddleware {
        CompressionMiddleware::with_config(self.config)
    }
}

impl Default for CompressionBuilder {
    fn default() -> Self {
        Self::new()
    }
}
