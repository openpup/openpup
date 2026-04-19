import { createInterface } from 'node:readline';
import type { AgentChatEnvelope } from './envelope.js';
import { fail, isObject, ok, optionalString, requiredString, requiredStringArray, type JsonValue, type RpcRequest } from './protocol.js';
import { XmtpClientFacade } from './xmtpClient.js';

function write(value: unknown) {
  process.stdout.write(`${JSON.stringify(value)}\n`);
}

function parseEnvelope(value: JsonValue | undefined): AgentChatEnvelope {
  if (!isObject(value)) throw new Error('missing envelope');
  return value as unknown as AgentChatEnvelope;
}

const helper = new XmtpClientFacade((event) => write(event));

async function handle(request: RpcRequest): Promise<JsonValue> {
  switch (request.method) {
    case 'init':
      return helper.init({
        env: optionalString(request.params, 'env') ?? 'dev',
        dataDir: requiredString(request.params, 'dataDir'),
        identityPrivateKey: requiredString(request.params, 'identityPrivateKey'),
        dbEncryptionKey: requiredString(request.params, 'dbEncryptionKey'),
      }) as Promise<JsonValue>;
    case 'status':
      return helper.status();
    case 'identity':
      return helper.identity();
    case 'createGroup':
      return helper.createGroup({
        conversationId: requiredString(request.params, 'conversationId'),
        title: requiredString(request.params, 'title'),
      }) as Promise<JsonValue>;
    case 'addMembers':
      return helper.addMembers({
        transportRef: requiredString(request.params, 'transportRef'),
        inboxIds: requiredStringArray(request.params, 'inboxIds'),
      }) as Promise<JsonValue>;
    case 'removeMembers':
      return helper.removeMembers({
        transportRef: requiredString(request.params, 'transportRef'),
        inboxIds: requiredStringArray(request.params, 'inboxIds'),
      }) as Promise<JsonValue>;
    case 'requestRemoval':
      return helper.requestRemoval({
        transportRef: requiredString(request.params, 'transportRef'),
      }) as Promise<JsonValue>;
    case 'syncGroups':
      return helper.syncGroups() as Promise<JsonValue>;
    case 'sendMessage':
      return helper.sendMessage({
        transportRef: requiredString(request.params, 'transportRef'),
        envelope: parseEnvelope(isObject(request.params) ? request.params.envelope : undefined),
      }) as Promise<JsonValue>;
    case 'startStream':
      return helper.startStream();
    case 'stopStream':
      return helper.stopStream();
    default:
      throw new Error(`unknown method: ${request.method}`);
  }
}

async function main() {
  const rl = createInterface({ input: process.stdin, crlfDelay: Infinity });
  for await (const line of rl) {
    const trimmed = line.trim();
    if (!trimmed) continue;
    let request: RpcRequest;
    try {
      request = JSON.parse(trimmed) as RpcRequest;
      if (!request.id || !request.method) throw new Error('invalid request');
    } catch (error) {
      process.stderr.write(`invalid JSONL request: ${String(error)}\n`);
      continue;
    }

    try {
      write(ok(request.id, await handle(request)));
    } catch (error) {
      write(fail(request.id, 'helper_error', error instanceof Error ? error.message : String(error)));
    }
  }
}

main().catch((error) => {
  process.stderr.write(`${error instanceof Error ? error.stack ?? error.message : String(error)}\n`);
  process.exitCode = 1;
});
