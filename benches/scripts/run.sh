#!/bin/bash
#
# Texide vs textlint Benchmark Runner
#
# Usage: ./benches/scripts/run.sh
# Output: benches/results/benchmark-$(date +%Y%m%d-%H%M%S).json
#

set -e

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
BENCH_DIR="$(dirname "$SCRIPT_DIR")"
PROJECT_DIR="$(dirname "$BENCH_DIR")"
RESULTS_DIR="$BENCH_DIR/results"
CORPUS_DIR="$BENCH_DIR/corpus"

# Ensure results directory exists
mkdir -p "$RESULTS_DIR"

# Generate timestamp
TIMESTAMP=$(date +%Y%m%d-%H%M%S)
RESULT_FILE="$RESULTS_DIR/benchmark-$TIMESTAMP.json"

echo "========================================"
echo "Texide vs textlint Benchmark"
echo "========================================"
echo "Timestamp: $TIMESTAMP"
echo "Results: $RESULT_FILE"
echo ""

# Step 1: Generate corpus if needed
if [ ! -d "$CORPUS_DIR/large_single.md" ] || [ ! -d "$CORPUS_DIR/many_files" ]; then
    echo "Step 1: Generating benchmark corpus..."
    cd "$PROJECT_DIR"
    cargo run --bin corpus-generator --release 2>&1 | tee "$RESULTS_DIR/corpus-generation.log"
    echo ""
else
    echo "Step 1: Corpus already exists, skipping generation"
    echo ""
fi

# Step 2: Build Texide in release mode
echo "Step 2: Building Texide (release mode)..."
cd "$PROJECT_DIR"
cargo build --bin texide --release 2>&1 | tee "$RESULTS_DIR/texide-build.log"
TEXIDE_BIN="$PROJECT_DIR/target/release/texide"
echo ""

# Step 3: Setup textlint
echo "Step 3: Setting up textlint..."
cd "$BENCH_DIR/rules/textlint"
if [ ! -d "node_modules" ]; then
    npm install 2>&1 | tee "$RESULTS_DIR/textlint-setup.log"
fi
echo ""

# Step 4: Run benchmarks
echo "Step 4: Running benchmarks..."
echo ""

# Initialize results JSON
cat > "$RESULT_FILE" << 'EOF'
{
  "timestamp": "TIMESTAMP_PLACEHOLDER",
  "texide_version": "TEXIDE_VERSION_PLACEHOLDER",
  "textlint_version": "TEXTLINT_VERSION_PLACEHOLDER",
  "system_info": {
    "os": "OS_PLACEHOLDER",
    "cpu": "CPU_PLACEHOLDER",
    "memory": "MEMORY_PLACEHOLDER"
  },
  "scenarios": []
}
EOF

# Replace placeholders with actual values
TIMESTAMP=$(date -u +"%Y-%m-%dT%H:%M:%SZ")
sed -i.bak "s/TIMESTAMP_PLACEHOLDER/$TIMESTAMP/" "$RESULT_FILE"

# Get Texide version
TEXIDE_VERSION=$($TEXIDE_BIN --version 2>/dev/null || echo "unknown")
sed -i.bak "s/TEXIDE_VERSION_PLACEHOLDER/$TEXIDE_VERSION/" "$RESULT_FILE"

# Get textlint version
cd "$BENCH_DIR/rules/textlint"
TEXTLINT_VERSION=$(npx textlint --version 2>/dev/null || echo "unknown")
cd "$PROJECT_DIR"
sed -i.bak "s/TEXTLINT_VERSION_PLACEHOLDER/$TEXTLINT_VERSION/" "$RESULT_FILE"

# Get system info
OS=$(uname -s -r)
CPU=$(sysctl -n machdep.cpu.brand_string 2>/dev/null || grep 'model name' /proc/cpuinfo | head -1 | cut -d':' -f2 | xargs || echo "unknown")
MEMORY=$(sysctl -n hw.memsize 2>/dev/null | awk '{print $1/1024/1024/1024 " GB"}' || grep MemTotal /proc/meminfo | awk '{print $2/1024/1024 " GB"}' || echo "unknown")
sed -i.bak "s|OS_PLACEHOLDER|$OS|" "$RESULT_FILE"
sed -i.bak "s|CPU_PLACEHOLDER|$CPU|" "$RESULT_FILE"
sed -i.bak "s|MEMORY_PLACEHOLDER|$MEMORY|" "$RESULT_FILE"
rm -f "$RESULT_FILE.bak"

# Scenario 1: Single large file (100MB)
echo "Scenario 1: Large Single File (100MB)"
echo "  File: corpus/large_single.md"
echo ""

# Texide - Cold start (first run, no cache)
echo "  Texide - Cold Start..."
cd "$PROJECT_DIR"
rm -rf .texide-cache  # Clear cache for cold start test
TEXIDE_COLD_START=$({
    TIMEFORMAT='%R'
    time ($TEXIDE_BIN lint "$CORPUS_DIR/large_single.md" --rules benches/rules/texide/no-todo 2>&1 >/dev/null)
} 2>&1)
echo "    Cold start time: ${TEXIDE_COLD_START}s"

# Texide - Warm run (with cache)
echo "  Texide - Warm Run..."
TEXIDE_WARM_TIME=$({
    TIMEFORMAT='%R'
    time ($TEXIDE_BIN lint "$CORPUS_DIR/large_single.md" --rules benches/rules/texide/no-todo 2>&1 >/dev/null)
} 2>&1)
echo "    Warm run time: ${TEXIDE_WARM_TIME}s"

# Texide - Peak memory (using time command)
echo "  Texide - Memory Usage..."
if command -v /usr/bin/time >/dev/null 2>&1; then
    TEXIDE_MEMORY=$(/usr/bin/time -v $TEXIDE_BIN lint "$CORPUS_DIR/large_single.md" --rules benches/rules/texide/no-todo 2>&1 | grep "Maximum resident" | awk '{print $6}' || echo "0")
else
    TEXIDE_MEMORY="N/A"
fi
echo "    Peak memory: ${TEXIDE_MEMORY} KB"
echo ""

# textlint
echo "  textlint..."
cd "$BENCH_DIR/rules/textlint"
TEXTLINT_TIME=$({
    TIMEFORMAT='%R'
    time (npx textlint "$CORPUS_DIR/large_single.md" 2>&1 >/dev/null)
} 2>&1)
echo "    Execution time: ${TEXTLINT_TIME}s"
echo ""

# Scenario 2: Many small files (1000 files)
echo "Scenario 2: Many Small Files (1000 files)"
echo "  Directory: corpus/many_files/"
echo ""

# Texide - Cold start
echo "  Texide - Cold Start..."
cd "$PROJECT_DIR"
rm -rf .texide-cache
TEXIDE_MANY_COLD=$({
    TIMEFORMAT='%R'
    time ($TEXIDE_BIN lint "$CORPUS_DIR/many_files/" --rules benches/rules/texide/no-todo 2>&1 >/dev/null)
} 2>&1)
echo "    Cold start time: ${TEXIDE_MANY_COLD}s"

# Texide - Warm run
echo "  Texide - Warm Run..."
TEXIDE_MANY_WARM=$({
    TIMEFORMAT='%R'
    time ($TEXIDE_BIN lint "$CORPUS_DIR/many_files/" --rules benches/texide/no-todo 2>&1 >/dev/null)
} 2>&1)
echo "    Warm run time: ${TEXIDE_MANY_WARM}s"
echo ""

# textlint
echo "  textlint..."
cd "$BENCH_DIR/rules/textlint"
TEXTLINT_MANY_TIME=$({
    TIMEFORMAT='%R'
    time (npx textlint "$CORPUS_DIR/many_files/" 2>&1 >/dev/null)
} 2>&1)
echo "    Execution time: ${TEXTLINT_MANY_TIME}s"
echo ""

# Generate final JSON results
node "$SCRIPT_DIR/generate-report.js" \
    "$RESULT_FILE" \
    "$TEXIDE_COLD_START" \
    "$TEXIDE_WARM_TIME" \
    "$TEXIDE_MEMORY" \
    "$TEXTLINT_TIME" \
    "$TEXIDE_MANY_COLD" \
    "$TEXIDE_MANY_WARM" \
    "$TEXTLINT_MANY_TIME"

echo "========================================"
echo "Benchmark Complete!"
echo "Results: $RESULT_FILE"
echo "========================================"
