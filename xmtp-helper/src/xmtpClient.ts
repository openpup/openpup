import { mkdir, readFile, writeFile } from 'node:fs/promises';
import { join } from 'node:path';
import { privateKeyToAccount } from 'viem/accounts';
import type { Hex } from 'viem';
import type { Client as XmtpClient, Group, Signer, XmtpEnv } from '@xmtp/node-sdk';
import type { AgentChatEnvelope } from './envelope.js';
import type { HelperEvent, JsonValue } from './protocol.js';

interface HelperState {
  env: string;
  inboxId: string;
  groups: Record<string, { title: string; createdAt: number }>;
}

export interface InitParams {
  env: string;
  dataDir: string;
  identityPrivateKey: string;
  dbEncryptionKey: string;
}

export interface CreateGroupParams {
  conversationId: string;
  title: string;
}

export interface SendMessageParams {
  transportRef: string;
  envelope: AgentChatEnvelope;
}

export interface AddMembersParams {
  transportRef: string;
  inboxIds: string[];
}

export interface RemoveMembersParams {
  transportRef: string;
  inboxIds: string[];
}

export interface RequestRemovalParams {
  transportRef: string;
}

export class XmtpClientFacade {
  private dataDir = '';
  private statePath = '';
  private state: HelperState | null = null;
  private client: XmtpClient<any> | null = null;
  private groups = new Map<string, Group<any>>();
  private streamActive = false;
  private messageStreamRunning = false;
  private groupStreamRunning = false;

  constructor(private readonly emit: (event: HelperEvent) => void) {}

  async init(params: InitParams) {
    this.dataDir = params.dataDir;
    this.statePath = join(this.dataDir, 'helper-state.json');
    await mkdir(this.dataDir, { recursive: true });
    this.state = await this.loadState(params.env);
    try {
      await this.initRealClient(params);
    } catch (error) {
      this.client = null;
      this.state = null;
      throw error;
    }
    await this.saveState();
    this.emit({
      event: 'status',
      payload: {
        status: 'ready',
        env: this.state.env,
        inboxId: this.state.inboxId,
        mode: 'xmtp',
      },
    });
    return {
      env: this.state.env,
      inboxId: this.state.inboxId,
      mode: 'xmtp',
      dataDir: this.dataDir,
    };
  }

  status() {
    this.assertReady();
    return {
      status: 'ready',
      env: this.state!.env,
      inboxId: this.state!.inboxId,
      groupCount: Object.keys(this.state!.groups).length,
      mode: 'xmtp',
    };
  }

  identity() {
    this.assertReady();
    return {
      env: this.state!.env,
      inboxId: this.state!.inboxId,
    };
  }

  async createGroup(params: CreateGroupParams) {
    this.assertReady();
    const group = await this.client!.conversations.createGroup([], {
      groupName: params.title,
      groupDescription: `OpenPup Conversation Space ${params.conversationId}`,
      appData: JSON.stringify({
        protocol: 'agent-conversation-v1',
        localHint: params.conversationId,
      }),
    });
    this.groups.set(group.id, group);
    this.state!.groups[group.id] = {
      title: params.title,
      createdAt: Math.floor(Date.now() / 1000),
    };
    await this.saveState();
    return {
      transportRef: group.id,
    };
  }

  async sendMessage(params: SendMessageParams) {
    this.assertReady();
    const group = await this.resolveGroup(params.transportRef);
    const remoteMessageId = await group.sendText(JSON.stringify(params.envelope));
    return {
      remoteMessageId,
      transportRef: params.transportRef,
    };
  }

  async addMembers(params: AddMembersParams) {
    this.assertReady();
    const group = await this.resolveGroup(params.transportRef);
    await group.addMembers(params.inboxIds);
    await group.sync();
    return {
      transportRef: params.transportRef,
      inboxIds: params.inboxIds,
    };
  }

  async removeMembers(params: RemoveMembersParams) {
    this.assertReady();
    const group = await this.resolveGroup(params.transportRef);
    await group.removeMembers(params.inboxIds);
    await group.sync();
    return {
      transportRef: params.transportRef,
      inboxIds: params.inboxIds,
    };
  }

  async requestRemoval(params: RequestRemovalParams) {
    this.assertReady();
    const group = await this.resolveGroup(params.transportRef);
    await group.requestRemoval();
    await group.sync();
    return {
      transportRef: params.transportRef,
      pendingRemoval: group.isPendingRemoval(),
    };
  }

  async syncGroups() {
    this.assertReady();
    await this.client!.conversations.syncAll();
    const groups = this.client!.conversations.listGroups();
    const payload = groups.map((group) => this.describeGroup(group));
    await this.saveState();
    for (const group of payload) {
      this.emit({ event: 'group', payload: group });
    }
    return { groups: payload };
  }

  startStream() {
    this.assertReady();
    if (!this.streamActive) {
      this.streamActive = true;
    }
    if (!this.messageStreamRunning) {
      void this.streamRealMessages();
    }
    if (!this.groupStreamRunning) {
      void this.streamRealGroups();
    }
    this.emit({
      event: 'stream',
      payload: {
        status: 'started',
        mode: 'xmtp',
      },
    });
    return { status: 'started' };
  }

  stopStream() {
    this.assertReady();
    this.streamActive = false;
    this.emit({
      event: 'stream',
      payload: {
        status: 'stopped',
        mode: 'xmtp',
      },
    });
    return { status: 'stopped' };
  }

  private async initRealClient(params: InitParams) {
    if (this.client) return;
    if (!params.identityPrivateKey || !params.dbEncryptionKey) {
      throw new Error('missing XMTP identity');
    }

    const account = privateKeyToAccount(normalizePrivateKey(params.identityPrivateKey));
    const signer: Signer = {
      type: 'EOA',
      getIdentifier: () => ({
        identifier: account.address.toLowerCase(),
        identifierKind: 0,
      }),
      signMessage: async (message) => hexToBytes(await account.signMessage({ message })),
    };
    const { Client } = await import('@xmtp/node-sdk');
    const client = await Client.create(signer, {
      env: normalizeEnv(params.env),
      dbEncryptionKey: hexToBytes(params.dbEncryptionKey),
      dbPath: join(this.dataDir, 'xmtp.db3'),
    } as Parameters<typeof Client.create>[1]);
    this.client = client;
    this.state!.inboxId = client.inboxId;
    await client.conversations.sync();
  }

  private async resolveGroup(transportRef: string): Promise<Group<any>> {
    if (!this.client) throw new Error('XMTP client is not initialized');
    const cached = this.groups.get(transportRef);
    if (cached) return cached;
    const conversation = await this.client.conversations.getConversationById(transportRef);
    if (!conversation || !('name' in conversation)) {
      throw new Error(`unknown XMTP group: ${transportRef}`);
    }
    this.groups.set(transportRef, conversation);
    return conversation;
  }

  private async streamRealMessages() {
    if (!this.client) return;
    this.messageStreamRunning = true;
    let retryDelayMs = 1000;
    try {
      while (this.streamActive && this.client) {
        try {
          await this.client.conversations.syncAll();
          const stream = await this.client.conversations.streamAllGroupMessages();
          retryDelayMs = 1000;
          for await (const message of stream) {
            if (!this.streamActive) {
              await stream.return();
              break;
            }
            if (message.senderInboxId === this.client.inboxId) {
              continue;
            }
            const rawText = typeof message.content === 'string' ? message.content : message.fallback ?? '';
            let envelope: JsonValue = null;
            try {
              envelope = rawText ? JSON.parse(rawText) as JsonValue : null;
            } catch {
              envelope = null;
            }
            this.emit({
              event: 'message',
              payload: {
                transportRef: message.conversationId,
                remoteMessageId: message.id,
                envelope,
              },
            });
          }
          if (this.streamActive) {
            await delay(1000);
          }
        } catch (error) {
          if (!this.streamActive) break;
          this.emit({
            event: 'stream',
            payload: {
              status: 'reconnecting',
              target: 'messages',
              mode: 'xmtp',
              error: error instanceof Error ? error.message : String(error),
            },
          });
          await delay(retryDelayMs);
          retryDelayMs = Math.min(retryDelayMs * 2, 30000);
        }
      }
    } finally {
      this.messageStreamRunning = false;
    }
  }

  private async streamRealGroups() {
    if (!this.client) return;
    this.groupStreamRunning = true;
    let retryDelayMs = 1000;
    try {
      while (this.streamActive && this.client) {
        try {
          await this.client.conversations.sync();
          const stream = await this.client.conversations.streamGroups();
          retryDelayMs = 1000;
          for await (const group of stream) {
            if (!this.streamActive) {
              await stream.return();
              break;
            }
            this.groups.set(group.id, group);
            this.state!.groups[group.id] = {
              title: group.name || 'XMTP Group',
              createdAt: Math.floor(group.createdAt.getTime() / 1000),
            };
            await this.saveState();
            this.emit({
              event: 'group',
              payload: this.describeGroup(group),
            });
          }
          if (this.streamActive) {
            await delay(1000);
          }
        } catch (error) {
          if (!this.streamActive) break;
          this.emit({
            event: 'stream',
            payload: {
              status: 'reconnecting',
              target: 'groups',
              mode: 'xmtp',
              error: error instanceof Error ? error.message : String(error),
            },
          });
          await delay(retryDelayMs);
          retryDelayMs = Math.min(retryDelayMs * 2, 30000);
        }
      }
    } finally {
      this.groupStreamRunning = false;
    }
  }

  private describeGroup(group: Group<any>) {
    this.groups.set(group.id, group);
    this.state!.groups[group.id] = {
      title: group.name || 'XMTP Group',
      createdAt: Math.floor(group.createdAt.getTime() / 1000),
    };
    return {
      transportRef: group.id,
      title: group.name || 'XMTP Group',
      description: group.description || '',
      createdAt: Math.floor(group.createdAt.getTime() / 1000),
      addedByInboxId: group.addedByInboxId || '',
    };
  }

  private async loadState(env: string): Promise<HelperState> {
    try {
      return JSON.parse(await readFile(this.statePath, 'utf8')) as HelperState;
    } catch {
      return {
        env,
        inboxId: '',
        groups: {},
      };
    }
  }

  private async saveState() {
    await writeFile(this.statePath, JSON.stringify(this.state, null, 2), 'utf8');
  }

  private assertReady() {
    if (!this.state) {
      throw new Error('helper is not initialized');
    }
  }
}

function normalizePrivateKey(privateKey: string): Hex {
  return privateKey.startsWith('0x') ? privateKey as Hex : `0x${privateKey}` as Hex;
}

function hexToBytes(hex: string): Uint8Array {
  const clean = hex.startsWith('0x') ? hex.slice(2) : hex;
  const bytes = new Uint8Array(clean.length / 2);
  for (let i = 0; i < bytes.length; i += 1) {
    bytes[i] = Number.parseInt(clean.slice(i * 2, i * 2 + 2), 16);
  }
  return bytes;
}

function normalizeEnv(env: string): XmtpEnv {
  if (env === 'production' || env === 'mainnet') return 'production';
  if (env === 'local') return 'local';
  return 'dev';
}

function delay(ms: number) {
  return new Promise((resolve) => setTimeout(resolve, ms));
}
