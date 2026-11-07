/** Device Center IPC v1 host contract. Types only; DC payloads retain their serde shapes. */
export type DcModuleState = 'absent' | 'untrusted' | 'stopped' | 'starting' | 'running' | 'reconnecting' | 'elsewhere' | 'incompatible' | 'error';
export interface DcProtocol { major: number; minor: number }
export interface DcModuleStatus {
  state: DcModuleState;
  pid: number | null;
  protocol: DcProtocol | null;
  moduleVersion: string | null;
  capabilities: DcMethod[];
  keep: boolean;
  message: string | null;
  reconnectAttempt: 1 | 2 | 3 | null;
  exitCode: number | null;
}

export interface DcHidDevice { id: string; vendorId: number; productId: number; usagePage: number; usage: number; interfaceNumber: number; manufacturer: string | null; product: string | null; batterySupported: boolean }
export interface DcBattery { deviceId: string; battery?: number | null; batteryPercent?: number | null; percentage?: number | null; connection?: string; stale?: boolean; updatedAt?: number }
export interface DcSnapshots { devices: DcBattery[]; lamp: (Partial<DcLampState> & { connection?: string; stale?: boolean }) | null }
export interface DcField { key: string; label: string; kind: string; value: number | null; available: boolean; reason: string | null; min: number | null; max: number | null; step: number | null; options: {label: string; value: number}[] | null }
export interface DcSettings { deviceId: string; model: string; fields: DcField[]; sampledAt: number; verifiedOnHardware: boolean; stateToken: string | null }
export interface DcSettingsChange { reportRateHz?: number; dpiLevel?: number; sleepSeconds?: number; lowLatency?: boolean; macMode?: boolean; winLock?: boolean; expectedReportRateHz?: number; expectedDpiLevel?: number; expectedState?: string }
export interface DcDpi { deviceId: string; model: string; sensor: string; stateToken: string; currentLevel: number; levelCount: number; levels: {level: number; xDpi: number; yDpi: number; editable: boolean}[]; min: number; max: number; stepSegments: {until:number;step:number}[]; verifiedOnHardware: boolean }
export interface DcDpiChange {level:number; xDpi:number; yDpi:number; expectedState:string}
export interface DcLighting { deviceId: string; model: string; sampledAt: number; stateToken: string; mode: number; brightness: number | null; speed: number | null; multiColor: boolean | null; rgb: [number,number,number] | null; colorError: string | null; modes: {id:number;label:string;brightnessEditable:boolean;speedEditable:boolean;colorEditable:boolean}[]; unsupported:string[]; verifiedOnHardware:boolean }
export interface DcLightingChange { expectedState: string; mode?:number; brightness?:number; speed?:number; multiColor?:boolean; rgb?:[number,number,number] }
export interface DcLampConfig {configured:boolean;ip:string|null;tokenTail:string|null}
export interface DcLampState {power:boolean;brightness:number;temperature:number;sampledAt:number}
export interface DcLampChange {power?:boolean;brightness?:number;temperature?:number}
export interface DcShortcut {keyCode:number;modifiers:number[]}
export interface DcShortcutSupport {maxModifiers:number;maxActions:number;keyCodeMin:number;keyCodeMax:number;modifiers:number[];keyboardOptions:{label:string;value:number;category:string}[]}
export interface DcMapping {deviceId:string;model:string;sampledAt:number;stateToken:string;buttons:{id:string;label:string;currentAction:string|null;currentLabel:string;editable:boolean;rawHex:string;shortcut:DcShortcut|null}[];actions:{id:string;label:string;category:string}[];unsupported:{id:string;reason:string}[];shortcutSupport:DcShortcutSupport|null}
export interface DcMappingChange {buttonId:string;action:string;expectedState:string;shortcut?:DcShortcut}
export type DcMethod = 'device.list' | 'device.snapshots' | 'battery.read' | 'settings.read' | 'settings.apply'
  | 'mouse.dpi.read' | 'mouse.dpi.apply' | 'mouse.mapping.read' | 'mouse.mapping.apply'
  | 'keyboard.lighting.read' | 'keyboard.lighting.apply' | 'lamp.config.read' | 'lamp.config.save' | 'lamp.read' | 'lamp.apply';
export type DcErrorCode = 'PROTOCOL_MISMATCH' | 'UNAUTHORIZED' | 'UNKNOWN_METHOD' | 'INVALID_PARAMS'
  | 'MODULE_UNTRUSTED' | 'DEVICE_DISCONNECTED' | 'BUSY' | 'TIMEOUT' | 'STALE_STATE' | 'READBACK_MISMATCH' | 'PARTIAL_WRITE' | 'INTERNAL';
/** Write TIMEOUT or PARTIAL_WRITE means “结果未知，请刷新读回”; no success or rollback claim. */
export interface DcError { code: DcErrorCode; message: string; retryable: boolean; details: unknown }
export type DcCallResult<T = unknown> = { ok: true; result: T } | { ok: false; error: DcError };
export type DcReadback<T = unknown> = { deviceId: string | null; method: DcMethod } & DcCallResult<T>;
export interface DcEvent {
  type: 'event'; seq: number;
  name: 'battery.updated' | 'lamp.updated' | 'device.changed' | 'module.status';
  deviceId?: string;
  data: unknown;
}
