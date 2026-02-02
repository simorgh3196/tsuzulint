import { workspace, ExtensionContext } from 'vscode';
import {
  LanguageClient,
  LanguageClientOptions,
  ServerOptions,
  Trace,
} from 'vscode-languageclient/node';

let client: LanguageClient;

/**
 * Maps the trace.server configuration string to the Trace enum.
 */
function getTraceLevel(traceValue: string | undefined): Trace {
  switch (traceValue) {
    case 'messages':
      return Trace.Messages;
    case 'verbose':
      return Trace.Verbose;
    case 'off':
    default:
      return Trace.Off;
  }
}

export function activate(context: ExtensionContext) {
  const config = workspace.getConfiguration('tsuzulint');
  const command = config.get<string>('executablePath', 'tzlint');
  const traceServer = config.get<string>('trace.server');

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
      fileEvents: workspace.createFileSystemWatcher('**/.tzlint.{json,jsonc}'),
    },
  };

  client = new LanguageClient(
    'tsuzulint',
    'TsuzuLint Language Server',
    serverOptions,
    clientOptions
  );

  // Set trace level from configuration
  client.setTrace(getTraceLevel(traceServer));

  client.start();
}

export function deactivate(): Thenable<void> | undefined {
  if (!client) {
    return undefined;
  }
  return client.stop();
}
