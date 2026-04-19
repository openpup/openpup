declare module 'node:fs/promises' {
  export function mkdir(path: string, options?: { recursive?: boolean }): Promise<void>;
  export function readFile(path: string, encoding: 'utf8'): Promise<string>;
  export function writeFile(path: string, data: string, options?: 'utf8' | { encoding?: 'utf8'; mode?: number }): Promise<void>;
}

declare module 'node:path' {
  export function join(...parts: string[]): string;
}

declare module 'node:readline' {
  export function createInterface(options: {
    input: unknown;
    crlfDelay?: number;
  }): AsyncIterable<string> & { close(): void };
}

declare const process: {
  stdin: unknown;
  stdout: { write(chunk: string): void };
  stderr: { write(chunk: string): void };
  env: Record<string, string | undefined>;
  exitCode?: number;
};
