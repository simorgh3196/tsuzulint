/**
 * {{RULE_NAME}} rule: {{RULE_DESCRIPTION}}
 *
 * This is an AssemblyScript implementation of a TsuzuLint rule.
 */

import { JSON } from "json-as";
import { Host, Output } from "@aspect/as-pdk";

// ============================================================================
// Type Definitions (generated from schemas/rule-types.json)
// ============================================================================

@json
class Manifest {
  name: string = "{{RULE_NAME}}";
  version: string = "0.1.0";
  description: string = "{{RULE_DESCRIPTION}}";
  fixable: boolean = false;
  node_types: string[] = ["Str"];
}

@json
class Span {
  start: u32 = 0;
  end: u32 = 0;
}

@json
class Diagnostic {
  rule_id: string = "";
  message: string = "";
  span: Span = new Span();
  severity: string = "warning";
}

@json
class LintResponse {
  diagnostics: Diagnostic[] = [];
}

@json
class AstNode {
  type: string = "";
  range: i32[] = [];
}

@json
class LintRequest {
  node: AstNode = new AstNode();
  config: Map<string, string> = new Map();
  source: string = "";
  file_path: string | null = null;
}

// ============================================================================
// Rule Configuration
// ============================================================================

@json
class Config {
  example_option: string = "default";
}

// ============================================================================
// Exported Functions
// ============================================================================

/**
 * Returns rule metadata.
 */
export function get_manifest(): i32 {
  const manifest = new Manifest();
  Output.setString(JSON.stringify(manifest));
  return 0;
}

/**
 * Lints a single AST node.
 */
export function lint(): i32 {
  const input = Host.inputString();
  const request = JSON.parse<LintRequest>(input);
  const diagnostics: Diagnostic[] = [];

  // Only process Str nodes
  if (request.node.type != "Str") {
    const response = new LintResponse();
    response.diagnostics = diagnostics;
    Output.setString(JSON.stringify(response));
    return 0;
  }

  // Extract text range
  if (request.node.range.length >= 2) {
    const start = request.node.range[0];
    const end = request.node.range[1];
    const text = request.source.substring(start, end);

    // TODO: Implement your lint logic here
    //
    // Example: Check for a specific pattern
    // if (text.includes("BAD_PATTERN")) {
    //   const diag = new Diagnostic();
    //   diag.rule_id = "{{RULE_NAME}}";
    //   diag.message = "Found bad pattern in text";
    //   diag.span.start = start as u32;
    //   diag.span.end = end as u32;
    //   diag.severity = "warning";
    //   diagnostics.push(diag);
    // }

    // Placeholder to avoid unused variable warning
    if (text.length == 0) {}
  }

  const response = new LintResponse();
  response.diagnostics = diagnostics;
  Output.setString(JSON.stringify(response));
  return 0;
}
