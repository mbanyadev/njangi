import { Buffer } from "buffer";
(globalThis as any).Buffer ??= Buffer;
(globalThis as any).global ??= globalThis;
(globalThis as any).process ??= { env: {} };
