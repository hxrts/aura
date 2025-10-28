#!/bin/bash
# Build script for optimized WASM output

set -e

echo "Building Aura Simulation Client WASM module..."

# Build with wasm-pack
wasm-pack build \
    --target web \
    --out-dir pkg \
    --release \
    --no-typescript \
    --no-pack

echo "Optimizing WASM bundle..."

# Optimize with wasm-opt if available
if command -v wasm-opt &> /dev/null; then
    echo "Running wasm-opt..."
    wasm-opt -Oz --enable-simd pkg/aura_sim_client_bg.wasm -o pkg/aura_sim_client_bg.wasm
else
    echo "wasm-opt not found, skipping optimization"
fi

# Check bundle size
if [ -f "pkg/aura_sim_client_bg.wasm" ]; then
    SIZE=$(wc -c < pkg/aura_sim_client_bg.wasm)
    SIZE_KB=$((SIZE / 1024))
    SIZE_GZIP=$(gzip -c pkg/aura_sim_client_bg.wasm | wc -c)
    SIZE_GZIP_KB=$((SIZE_GZIP / 1024))
    
    echo "WASM bundle size: ${SIZE_KB}KB (${SIZE_GZIP_KB}KB gzipped)"
    
    if [ $SIZE_GZIP_KB -gt 150 ]; then
        echo "WARNING: Bundle size (${SIZE_GZIP_KB}KB) exceeds target of 150KB"
    else
        echo "[OK] Bundle size within target range"
    fi
else
    echo "ERROR: WASM file not found"
    exit 1
fi

echo "Build complete!"
echo "Output files:"
ls -la pkg/