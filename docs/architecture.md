# TsuzuLint Architecture

This document describes the internal architecture of TsuzuLint.

## Overview

```mermaid
graph TB
    subgraph UI["User Interface"]
        CLI["CLI"]
        LSP["LSP"]
        Editor["Editor Plugins"]
    end

    subgraph Core["tsuzulint_core"]
        Linter["Linter Engine"]
        Config["Config Loader"]
        Cache["Cache Manager"]
        Parallel["Parallel Scheduler"]
    end

    subgraph Parser["tsuzulint_parser"]
        Markdown["Markdown (markdown-rs)"]
        PlainText["Plain Text"]
    end

    subgraph AST["tsuzulint_ast"]
        TxtNode["TxtNode<'a>"]
        Arena["AstArena (bumpalo)"]
    end

    subgraph Plugin["tsuzulint_plugin"]
        Host["PluginHost (Extism)"]
        Rule1["Rule 1 (WASM)"]
        Rule2["Rule 2 (WASM)"]
        Rule3["Rule 3 (WASM)"]
    end

    UI --> Core
    Core --> Parser
    Parser --> AST
    Core --> Plugin
    Plugin --> Rule1
    Plugin --> Rule2
    Plugin --> Rule3
```

## Crates

### tsuzulint_ast

**Purpose**: TxtAST type definitions and memory management.

**Key Components**:
- `TxtNode<'a>`: Arena-allocated AST node
- `AstArena`: bumpalo-based arena allocator
- `NodeType`: Node type enum (Document, Paragraph, Str, etc.)
- `Span`, `Position`: Source location types

**Design Decisions**:
- Uses bumpalo for arena allocation (inspired by Oxc)
- All nodes for a file share one arena
- No `Box`, `Rc`, or `Arc` for child nodes
- Maximizes cache locality

### tsuzulint_parser

**Purpose**: Parse source text into TxtAST.

**Key Components**:
- `Parser` trait: Abstraction for parsers
- `MarkdownParser`: Markdown parser using markdown-rs
- `PlainTextParser`: Simple text parser

**Design Decisions**:
- Uses markdown-rs for mdast output (minimal transform to TxtAST)
- Parser trait enables custom parsers via WASM plugins
- Each parser handles specific file extensions

### tsuzulint_plugin

**Purpose**: WASM plugin system for rules.

**Key Components**:
- `PluginHost`: Extism-based WASM runtime
- `RuleManifest`: Rule metadata
- `Diagnostic`: Lint result with fix information

**Design Decisions**:
- Extism provides sandboxing and multi-language support
- Rules compile to `wasm32-wasip1`
- JSON serialization for host↔plugin communication
- Potential future migration to direct wasmtime use

### tsuzulint_cache

**Purpose**: Cache lint results for unchanged files.

**Key Components**:
- `CacheManager`: File I/O and cache validation
- `CacheEntry`: Cached lint result

**Design Decisions**:
- BLAKE3 for content hashing
- JSON storage (rkyv planned for zero-copy)
- Invalidates on: content change, config change, rule version change

### tsuzulint_core

**Purpose**: Core linter orchestration.

**Key Components**:
- `Linter`: Main linter engine
- `LinterConfig`: Configuration handling
- `LintResult`: Per-file lint result

**Design Decisions**:
- Uses rayon for parallel file processing
- Glob patterns for file discovery
- Integrates parsers, plugins, and cache

### tsuzulint_cli

**Purpose**: Command-line interface.

**Commands**:
- `lint`: Lint files
- `init`: Create config file
- `create-rule`: Generate rule project
- `add-rule`: Register a WASM rule

### tsuzulint_lsp

**Purpose**: Language Server Protocol implementation.

**Status**: 🚧 β版 - Basic implementation (Diagnostics, Code Actions, Symbols) completed.

**Details**: See [LSP Documentation](lsp.md) for more information.

## Data Flow

```mermaid
flowchart TD
    A[CLI receives file patterns] --> B[Linter discovers files using glob matching]
    B --> C{For each file - parallel}
    C --> D{Check cache validity}
    D -->|Valid| E[Return cached result]
    D -->|Invalid| F[Read file content]
    F --> G[Select parser by extension]
    G --> H[Parse to TxtAST - arena-allocated]
    H --> I[Convert AST to JSON]
    I --> J[Run each WASM rule]
    J --> K[Collect diagnostics]
    K --> L[Update cache]
    E --> M[Aggregate results]
    L --> M
    M --> N[Output - text/JSON format]
```

## Memory Model

```mermaid
graph TB
    subgraph Arena["AstArena"]
        subgraph Bump["Bump allocator (contiguous)"]
            Nodes["[TxtNode][TxtNode][TxtNode]..."]
            Strings["[String data][String data]..."]
        end
        Note1["No individual deallocation"]
        Note2["All freed at once when arena drops"]
        Note3["Excellent cache locality"]
    end
```

## WASM Plugin Interface

```mermaid
sequenceDiagram
    participant Host as Host (Rust)
    participant Rule as WASM Rule (Sandbox)

    Host->>Rule: get_manifest()
    Rule-->>Host: manifest JSON

    Host->>Rule: lint(AST JSON, context, source, config)
    Rule-->>Host: diagnostics JSON
```

## Performance Considerations

1. **Arena Allocation**: Minimizes allocation overhead
2. **Parallel Processing**: rayon for multi-file linting
3. **Caching**: Skip unchanged files
4. **WASM Pre-compilation**: Extism caches compiled modules
5. **Lazy Parsing**: Only parse when cache miss

## Future Enhancements

- [ ] Line-level incremental caching
- [ ] Hot-reload for rules
- [ ] IDE plugin development
- [ ] Rule dependency graph
- [ ] Performance benchmarks vs textlint
