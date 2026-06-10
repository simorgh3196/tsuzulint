# JSON output format

`tzlint lint --format json` emits a stable, machine-readable report. It is the contract the
`editors/vscode/` extension parses (over `tzlint lint - --format json`, feeding the document on
stdin), so the shape below is treated as a stable interface: it evolves **additively** — new keys
may appear, but existing keys and their meanings do not change within a `0.x` minor series.

The text and SARIF formats (`--format text|sarif`) are documented separately; this page covers
only `--format json`.

## Shape

The top level is an array with one object per linted file, in the run's file order (a stdin
source is reported first, under the path `<stdin>`):

```jsonc
[
  {
    "path": "doc.md",            // the file path, or "<stdin>"
    "diagnostics": [ /* … */ ]   // empty array when the file is clean
  }
]
```

Each diagnostic:

| Field      | Type     | Meaning                                                                       |
| ---------- | -------- | ----------------------------------------------------------------------------- |
| `rule_id`  | string   | The rule that produced the diagnostic (e.g. `"no-todo"`).                     |
| `severity` | string   | One of `"error"`, `"warning"`, `"info"`, `"hint"` (always lowercase).         |
| `message`  | string   | The human-readable message.                                                   |
| `span`     | object   | The source range as **absolute byte offsets**: `{ "start", "end" }`, `end` exclusive. |
| `position` | object   | The same range mapped to line/column (see below).                             |
| `fixes`    | array    | Zero or more autofixes (see below); empty when the diagnostic is not fixable. |

A `position` (and every `fixes[].position`) has a `start` and an `end`, each a point:

| Field         | Type   | Meaning                                                                          |
| ------------- | ------ | -------------------------------------------------------------------------------- |
| `line`        | number | 1-based line.                                                                    |
| `column`      | number | 1-based column, counted in **Unicode scalar values** (matches the text format).  |
| `utf16Column` | number | 1-based column, counted in **UTF-16 code units** (a BMP char is 1, an astral-plane char is 2). Editors and the LSP address text in UTF-16. |

Each fix:

| Field         | Type   | Meaning                                                              |
| ------------- | ------ | ------------------------------------------------------------------- |
| `span`        | object | The byte range the fix replaces (`{ "start", "end" }`).             |
| `position`    | object | That range as line/column points (same shape as a diagnostic's).    |
| `replacement` | string | The text to substitute for `span`.                                  |

## Example

For the source `"あ x\n"` (the full-width `あ` is three UTF-8 bytes, so the `x` at byte offset 4
is at column 3):

```json
[
  {
    "path": "doc.md",
    "diagnostics": [
      {
        "rule_id": "no-todo",
        "severity": "warning",
        "message": "found x",
        "span": { "start": 4, "end": 5 },
        "position": {
          "start": { "line": 1, "column": 3, "utf16Column": 3 },
          "end": { "line": 1, "column": 4, "utf16Column": 4 }
        },
        "fixes": [
          {
            "span": { "start": 4, "end": 5 },
            "position": {
              "start": { "line": 1, "column": 3, "utf16Column": 3 },
              "end": { "line": 1, "column": 4, "utf16Column": 4 }
            },
            "replacement": "y"
          }
        ]
      }
    ]
  }
]
```

## Stability

The CLI and the editor integrations route through the same `tzlint_core::lint_document`, so an
editor sees byte-for-byte the diagnostics `tzlint lint` produces, in the same total order
(`span.start`, `span.end`, `rule_id`, `message`). A golden test (`json_contract_is_stable` in
`crates/tzlint_cli/src/output.rs`) pins this shape so an accidental change fails CI.
