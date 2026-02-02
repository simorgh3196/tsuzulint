# TsuzuLint for VS Code

This extension provides [TsuzuLint](https://github.com/simorgh3196/tsuzulint) support for Visual Studio Code.

## Features

- **Linting**: Real-time diagnostics for Markdown and Plain Text files.
- **Auto-fix**: Quick Fix actions for supported rules.

## Requirements

- **TsuzuLint CLI**: You need to have `tzlint` installed and available in your PATH, or specify the path in settings.

## Configuration

- `tsuzulint.executablePath`: Path to the `tzlint` executable. Default: `tzlint`.
- `tsuzulint.trace.server`: Enable debug logging for the language server.

## Development

1. Open this folder in VS Code.
2. Run `npm install`.
3. Press `F5` to start a Debugging session.
