import * as path from 'path';
import { workspace, ExtensionContext } from 'vscode';
import {
  LanguageClient,
  LanguageClientOptions,
  ServerOptions,
} from 'vscode-languageclient/node';

let client: LanguageClient;

export function activate(context: ExtensionContext) {
  const config = workspace.getConfiguration('texide');
  const command = config.get<string>('executablePath', 'texide');

  const serverOptions: ServerOptions = {
    run: { command, args: ['lsp'] },
    debug: { command, args: ['lsp'] }, // We might want to pass --verbose here if needed
  };

  const clientOptions: LanguageClientOptions = {
    documentSelector: [
      { scheme: 'file', language: 'markdown' },
      { scheme: 'file', language: 'plaintext' },
    ],
    synchronize: {
      fileEvents: workspace.createFileSystemWatcher('**/.texide.{json,jsonc}'),
    },
  };

  client = new LanguageClient(
    'texide',
    'Texide Language Server',
    serverOptions,
    clientOptions
  );

  client.start();
}

export function deactivate(): Thenable<void> | undefined {
  if (!client) {
    return undefined;
  }
  return client.stop();
}
