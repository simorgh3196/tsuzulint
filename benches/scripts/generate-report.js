#!/usr/bin/env node
/**
 * Generate benchmark report from results
 */

const fs = require('fs');
const path = require('path');

const [
    resultFile,
    texideColdStart,
    texideWarmTime,
    texideMemory,
    textlintTime,
    texideManyCold,
    texideManyWarm,
    textlintManyTime
] = process.argv.slice(2);

// Read existing template
let results = JSON.parse(fs.readFileSync(resultFile, 'utf8'));

// Parse numeric values (they might have units)
const parseTime = (val) => {
    const num = parseFloat(val);
    return isNaN(num) ? null : num;
};

const parseMemory = (val) => {
    if (val === 'N/A' || !val) return null;
    const num = parseInt(val);
    return isNaN(num) ? null : num;
};

// Add scenarios
results.scenarios = [
    {
        name: "large_single_file",
        description: "Single 100MB markdown file",
        file_count: 1,
        total_size_mb: 100,
        texide: {
            cold_start_seconds: parseTime(texideColdStart),
            warm_run_seconds: parseTime(texideWarmTime),
            peak_memory_kb: parseMemory(texideMemory)
        },
        textlint: {
            execution_seconds: parseTime(textlintTime),
            peak_memory_kb: null  // Would need additional instrumentation
        },
        speedup_cold: parseTime(textlintTime) / parseTime(texideColdStart),
        speedup_warm: parseTime(textlintTime) / parseTime(texideWarmTime)
    },
    {
        name: "many_small_files",
        description: "1000 small markdown files",
        file_count: 1000,
        total_size_mb: null,  // Would need calculation
        texide: {
            cold_start_seconds: parseTime(texideManyCold),
            warm_run_seconds: parseTime(texideManyWarm),
            peak_memory_kb: null
        },
        textlint: {
            execution_seconds: parseTime(textlintManyTime),
            peak_memory_kb: null
        },
        speedup_cold: parseTime(textlintManyTime) / parseTime(texideManyCold),
        speedup_warm: parseTime(textlintManyTime) / parseTime(texideManyWarm)
    }
];

// Write results
fs.writeFileSync(resultFile, JSON.stringify(results, null, 2));

console.log(`Results written to: ${resultFile}`);

// Print summary
console.log('\n=== Summary ===');
results.scenarios.forEach(scenario => {
    console.log(`\n${scenario.name}:`);
    console.log(`  Texide (cold): ${scenario.texide.cold_start_seconds?.toFixed(2)}s`);
    console.log(`  Texide (warm): ${scenario.texide.warm_run_seconds?.toFixed(2)}s`);
    console.log(`  textlint:      ${scenario.textlint.execution_seconds?.toFixed(2)}s`);
    console.log(`  Speedup:       ${scenario.speedup_warm?.toFixed(2)}x`);
});
