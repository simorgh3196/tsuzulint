/**
 * {{RULE_NAME}} rule: {{RULE_DESCRIPTION}}
 *
 * This is a TypeScript implementation of a Texide rule.
 * Compiled to WASM using Javy (https://github.com/bytecodealliance/javy).
 */

// ============================================================================
// Type Definitions
// ============================================================================

interface Manifest {
  name: string;
  version: string;
  description?: string;
  fixable: boolean;
  node_types: string[];
}

interface Span {
  start: number;
  end: number;
}

interface Fix {
  span: Span;
  text: string;
}

interface Diagnostic {
  rule_id: string;
  message: string;
  span: Span;
  severity: "error" | "warning" | "info";
  fix?: Fix;
}

interface AstNode {
  type: string;
  range: [number, number];
  children?: AstNode[];
}

interface LintHelpers {
  text?: string;
  location?: {
    start: { line: number; column: number };
    end: { line: number; column: number };
  };
  flags?: {
    in_code_block?: boolean;
    in_code_inline?: boolean;
    in_heading?: boolean;
    in_list?: boolean;
  };
}

interface LintRequest {
  node: AstNode;
  config: Record<string, unknown>;
  source: string;
  file_path?: string;
  helpers?: LintHelpers;
}

interface LintResponse {
  diagnostics: Diagnostic[];
}

// ============================================================================
// Rule Configuration
// ============================================================================

interface Config {
  exampleOption?: string;
}

const RULE_ID = "{{RULE_NAME}}";
const VERSION = "0.1.0";

// ============================================================================
// Javy Host Functions
// ============================================================================

// Javy provides global functions for I/O
declare const Javy: {
  IO: {
    readSync: () => Uint8Array;
    writeSync: (data: Uint8Array) => void;
  };
};

function readInput(): string {
  const bytes = Javy.IO.readSync();
  return new TextDecoder().decode(bytes);
}

function writeOutput(data: string): void {
  const bytes = new TextEncoder().encode(data);
  Javy.IO.writeSync(bytes);
}

// ============================================================================
// Rule Implementation
// ============================================================================

/**
 * Returns the rule manifest.
 */
function getManifest(): Manifest {
  return {
    name: RULE_ID,
    version: VERSION,
    description: "{{RULE_DESCRIPTION}}",
    fixable: false,
    node_types: ["Str"],
  };
}

/**
 * Lints a single AST node.
 */
function lint(request: LintRequest): LintResponse {
  const diagnostics: Diagnostic[] = [];

  // Only process Str nodes
  if (request.node.type !== "Str") {
    return { diagnostics };
  }

  // Parse configuration
  const config = request.config as Config;
  const _exampleOption = config.exampleOption ?? "default";

  // Extract text from node
  const [start, end] = request.node.range;
  const text = request.helpers?.text ?? request.source.slice(start, end);

  // Skip if inside code block
  if (request.helpers?.flags?.in_code_block) {
    return { diagnostics };
  }

  // TODO: Implement your lint logic here
  //
  // Example: Check for a specific pattern
  // if (text.includes("BAD_PATTERN")) {
  //   diagnostics.push({
  //     rule_id: RULE_ID,
  //     message: "Found bad pattern in text",
  //     span: { start, end },
  //     severity: "warning",
  //   });
  // }

  // Placeholder to avoid unused variable warning
  void text;
  void _exampleOption;

  return { diagnostics };
}

// ============================================================================
// Entry Point (Javy main function)
// ============================================================================

// Read input JSON, determine which function to call, execute, and write output
function main(): void {
  const rawInput = readInput();
  const trimmedInput = rawInput?.trim() || "";

  // Check if this is a manifest request or lint request
  // The host will call with empty input for get_manifest
  if (trimmedInput === "" || trimmedInput === "{}") {
    const manifest = getManifest();
    writeOutput(JSON.stringify(manifest));
  } else {
    try {
      const request: LintRequest = JSON.parse(trimmedInput);
      const response = lint(request);
      writeOutput(JSON.stringify(response));
    } catch (e) {
      // Return empty diagnostics on parse error
      writeOutput(JSON.stringify({ diagnostics: [] }));
    }
  }
}

main();
