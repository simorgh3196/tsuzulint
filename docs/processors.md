# Input formats

TsuzuLint lints Markdown by default. It can also lint **specific columns** of CSV/TSV files,
and the processor architecture lets new formats be added in-tree (see
[`design/input-format-processors.md`](design/input-format-processors.md) and the
`tzlint-processors` skill).

## CSV / TSV

Linting CSV/TSV is **opt-in per column**: only the columns you name are linted. Configure them
under `formats.csv` / `formats.tsv`:

```yaml
language: ja
rules:
  no-hankaku-kana: true        # base rules apply to every linted column unless overridden
formats:
  csv:
    header: true               # row 1 is a header (not linted); enables name lookup
    columns:
      title:                   # by header name
        parse-mode: plain      # treat the cell as plain text (default: markdown)
        rules:
          max-ten: { options: { max: 1 } }
      body:
        rules:                 # layered on top of the base rules (column wins)
          no-todo: true
  tsv:
    header: false
    columns:
      "2": { rules: { no-todo: true } }   # by 1-based column number
```

Notes:

- **Opt-in:** unlisted columns (ids, dates, …) are never linted.
- **Layering:** a column's effective rules are `base ⊕ column.rules`; set a rule to `false` in a
  column to drop it there.
- **Keys:** a string key is a header name (requires `header: true`); a bare integer key is a
  1-based column number.
- **delimiter:** defaults to `,` (csv) / tab (tsv); override with `delimiter: ";"`.
- **Discovery:** a directory/glob walk only picks up `.csv`/`.tsv` when the format is configured;
  an explicitly named file is always linted.
- **v1 limits:** an escaped quote (`""`) is linted as the raw `""`; TSV is treated as
  tab-delimited CSV.
