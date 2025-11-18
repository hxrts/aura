#!/bin/bash

# Aura-Sync Comprehensive Benchmark Runner
# 
# This script runs all benchmarking suites and generates comprehensive reports
# for performance analysis and tracking.

set -e

# Configuration
BENCHMARK_DIR="target/criterion"
REPORT_DIR="target/benchmark_reports"
TIMESTAMP=$(date +"%Y%m%d_%H%M%S")

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m' # No Color

echo -e "${BLUE}"
echo "╔══════════════════════════════════════════════════════════════════════════════╗"
echo "║                    AURA-SYNC PERFORMANCE BENCHMARK SUITE                    ║"
echo "║                                                                              ║"
echo "║  Comprehensive performance analysis for aura-sync protocols                 ║"
echo "║  Measuring throughput, latency, memory usage, and scaling behavior          ║"
echo "╚══════════════════════════════════════════════════════════════════════════════╝"
echo -e "${NC}"

# Create report directories
mkdir -p "$REPORT_DIR"
mkdir -p "$BENCHMARK_DIR"

# Function to run a benchmark suite with error handling
run_benchmark() {
    local benchmark_name=$1
    local description=$2
    
    echo -e "\n${YELLOW}🚀 Running $benchmark_name benchmark...${NC}"
    echo -e "   $description"
    echo "   ────────────────────────────────────────────────────────────────"
    
    if cargo bench --bench "$benchmark_name" 2>&1 | tee "$REPORT_DIR/${benchmark_name}_${TIMESTAMP}.log"; then
        echo -e "   ${GREEN}✅ $benchmark_name completed successfully${NC}"
    else
        echo -e "   ${RED}❌ $benchmark_name failed${NC}"
        return 1
    fi
}

# Function to generate HTML reports
generate_html_reports() {
    echo -e "\n${YELLOW}📊 Generating HTML reports...${NC}"
    
    if command -v criterion-html &> /dev/null; then
        criterion-html "$BENCHMARK_DIR" "$REPORT_DIR/html_reports_${TIMESTAMP}"
        echo -e "   ${GREEN}✅ HTML reports generated in $REPORT_DIR/html_reports_${TIMESTAMP}${NC}"
    else
        echo -e "   ${YELLOW}⚠️  criterion-html not found, skipping HTML report generation${NC}"
        echo -e "   Install with: cargo install criterion-html"
    fi
}

# Function to check system resources
check_system_resources() {
    echo -e "\n${BLUE}🔍 System Resource Check${NC}"
    echo "   ────────────────────────────────────────────────────────────────"
    
    # Check available memory
    if command -v free &> /dev/null; then
        echo "   Memory usage:"
        free -h
    elif command -v vm_stat &> /dev/null; then
        echo "   Memory usage (macOS):"
        vm_stat | head -5
    fi
    
    # Check CPU load
    if command -v uptime &> /dev/null; then
        echo -e "\n   System load:"
        uptime
    fi
    
    # Check available disk space
    echo -e "\n   Disk usage:"
    df -h . | head -2
    
    echo "   ────────────────────────────────────────────────────────────────"
}

# Function to create summary report
create_summary_report() {
    local summary_file="$REPORT_DIR/benchmark_summary_${TIMESTAMP}.md"
    
    echo -e "\n${YELLOW}📝 Creating summary report...${NC}"
    
    cat > "$summary_file" << EOF
# Aura-Sync Benchmark Summary Report

**Generated:** $(date)
**Timestamp:** $TIMESTAMP

## Test Environment

- **Host:** $(hostname)
- **OS:** $(uname -s) $(uname -r)
- **Architecture:** $(uname -m)
- **Rust Version:** $(rustc --version)
- **Cargo Version:** $(cargo --version)

## System Resources

EOF

    # Add system resource information
    if command -v free &> /dev/null; then
        echo "### Memory" >> "$summary_file"
        echo "\`\`\`" >> "$summary_file"
        free -h >> "$summary_file"
        echo "\`\`\`" >> "$summary_file"
        echo "" >> "$summary_file"
    fi
    
    if command -v lscpu &> /dev/null; then
        echo "### CPU Information" >> "$summary_file"
        echo "\`\`\`" >> "$summary_file"
        lscpu | head -10 >> "$summary_file"
        echo "\`\`\`" >> "$summary_file"
        echo "" >> "$summary_file"
    fi

    cat >> "$summary_file" << EOF

## Benchmark Suites Executed

1. **Sync Throughput** - Message throughput and processing rates
2. **Memory Usage** - Memory consumption patterns and leak detection  
3. **Protocol Latency** - End-to-end latency measurement
4. **Scaling Behavior** - Performance scaling with peers and operations
5. **Performance Report** - Baseline comparison and regression analysis
6. **Protocol Performance** - Integrated protocol validation

## Key Performance Insights

### Throughput Analysis
- Anti-entropy protocol: Digest creation and comparison efficiency
- Journal sync: Multi-peer synchronization rates
- Network utilization: Bandwidth efficiency and message optimization

### Memory Analysis
- Peak memory usage during sync operations
- Memory cleanup efficiency after protocol completion
- Large dataset handling capabilities

### Latency Analysis
- Round-trip protocol latency under various network conditions
- Impact of packet loss and jitter on performance
- Multi-peer coordination overhead

### Scaling Analysis
- Performance degradation with increasing peer counts
- Memory and CPU scaling characteristics
- Concurrent session handling capabilities

## Report Files

- **Raw benchmark data:** $BENCHMARK_DIR/
- **Detailed logs:** $REPORT_DIR/*_${TIMESTAMP}.log
- **HTML reports:** $REPORT_DIR/html_reports_${TIMESTAMP}/
- **Summary report:** $summary_file

## Performance Recommendations

Based on the benchmark results:

1. **Monitor** performance regressions in CI/CD pipelines
2. **Optimize** identified bottlenecks before production deployment  
3. **Scale** testing to match expected production loads
4. **Update** baselines after confirmed performance improvements

## Next Steps

1. Review detailed benchmark logs for specific performance metrics
2. Analyze HTML reports for visual performance trends
3. Compare results with previous benchmark runs
4. Implement optimizations for any identified performance issues

---

*Generated by Aura-Sync Benchmark Suite*
EOF

    echo -e "   ${GREEN}✅ Summary report created: $summary_file${NC}"
}

# Main execution
main() {
    local start_time=$(date +%s)
    local failed_benchmarks=0
    
    # Check system resources before starting
    check_system_resources
    
    echo -e "\n${BLUE}📋 Benchmark Execution Plan${NC}"
    echo "   ────────────────────────────────────────────────────────────────"
    echo "   1. Sync Throughput Benchmarks"
    echo "   2. Memory Usage Benchmarks" 
    echo "   3. Protocol Latency Benchmarks"
    echo "   4. Scaling Behavior Benchmarks"
    echo "   5. Performance Report Generation"
    echo "   6. Integrated Protocol Benchmarks"
    echo "   ────────────────────────────────────────────────────────────────"
    
    # Run benchmark suites
    run_benchmark "sync_throughput" "Measuring message throughput and processing rates" || ((failed_benchmarks++))
    run_benchmark "memory_usage" "Analyzing memory consumption and leak detection" || ((failed_benchmarks++))
    run_benchmark "protocol_latency" "Measuring end-to-end protocol latency" || ((failed_benchmarks++))
    run_benchmark "scaling_behavior" "Testing performance scaling with load" || ((failed_benchmarks++))
    run_benchmark "performance_report" "Generating baseline and regression analysis" || ((failed_benchmarks++))
    run_benchmark "protocol_performance" "Integrated protocol validation tests" || ((failed_benchmarks++))
    
    # Generate reports
    generate_html_reports
    create_summary_report
    
    # Final summary
    local end_time=$(date +%s)
    local duration=$((end_time - start_time))
    
    echo -e "\n${BLUE}🎯 Benchmark Execution Summary${NC}"
    echo "   ────────────────────────────────────────────────────────────────"
    echo -e "   Total execution time: ${YELLOW}${duration} seconds${NC}"
    echo -e "   Benchmarks completed: ${GREEN}$((6 - failed_benchmarks))/6${NC}"
    
    if [ $failed_benchmarks -eq 0 ]; then
        echo -e "   Status: ${GREEN}✅ All benchmarks completed successfully${NC}"
        echo -e "\n${GREEN}🎉 Benchmark suite completed successfully!${NC}"
        echo -e "   View results in: $REPORT_DIR/"
        exit 0
    else
        echo -e "   Status: ${RED}❌ $failed_benchmarks benchmark(s) failed${NC}"
        echo -e "\n${RED}⚠️  Some benchmarks failed. Check logs for details.${NC}"
        exit 1
    fi
}

# Handle script interruption
cleanup() {
    echo -e "\n${YELLOW}⚠️  Benchmark interrupted. Cleaning up...${NC}"
    exit 130
}

trap cleanup SIGINT SIGTERM

# Parse command line arguments
case "${1:-}" in
    --help|-h)
        echo "Usage: $0 [OPTIONS]"
        echo ""
        echo "OPTIONS:"
        echo "  --help, -h     Show this help message"
        echo "  --quick        Run quick benchmark suite (reduced samples)"
        echo "  --verbose      Enable verbose output"
        echo ""
        echo "Examples:"
        echo "  $0                 # Run full benchmark suite"
        echo "  $0 --quick         # Run quick benchmark suite"
        exit 0
        ;;
    --quick)
        echo -e "${YELLOW}⚡ Running quick benchmark suite (reduced sample sizes)${NC}"
        export CARGO_BENCH_OPTIONS="--sample-size 50"
        ;;
    --verbose)
        echo -e "${BLUE}🔍 Verbose mode enabled${NC}"
        set -x
        ;;
esac

# Run main function
main "$@"