export type JsonValue =
  | null
  | boolean
  | number
  | string
  | JsonValue[]
  | { [key: string]: JsonValue };

export interface RpcRequest {
  id: string;
  method: string;
  params?: JsonValue;
}

export interface RpcResponse {
  id: string;
  result?: JsonValue;
  error?: {
    code: string;
    message: string;
  };
}

export interface HelperEvent {
  event: string;
  payload: JsonValue;
}

export function ok(id: string, result: JsonValue): RpcResponse {
  return { id, result };
}

export function fail(id: string, code: string, message: string): RpcResponse {
  return { id, error: { code, message } };
}

export function isObject(value: JsonValue | undefined): value is { [key: string]: JsonValue } {
  return !!value && typeof value === 'object' && !Array.isArray(value);
}

export function requiredString(params: JsonValue | undefined, key: string): string {
  if (!isObject(params) || typeof params[key] !== 'string' || !params[key]) {
    throw new Error(`missing ${key}`);
  }
  return params[key];
}

export function optionalString(params: JsonValue | undefined, key: string): string | undefined {
  if (!isObject(params) || params[key] == null) return undefined;
  if (typeof params[key] !== 'string') throw new Error(`invalid ${key}`);
  return params[key];
}

export function requiredStringArray(params: JsonValue | undefined, key: string): string[] {
  if (!isObject(params) || !Array.isArray(params[key])) {
    throw new Error(`missing ${key}`);
  }
  const values = params[key];
  if (!values.every((value) => typeof value === 'string' && value.trim())) {
    throw new Error(`invalid ${key}`);
  }
  return values.map((value) => (value as string).trim());
}
