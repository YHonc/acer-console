import { invoke, isTauri } from '@tauri-apps/api/core';

// Wire contract: Rust commands use snake_case JSON fields and camelCase invoke arguments.
export type Profile = 'quiet' | 'balanced' | 'performance' | 'turbo' | 'eco';
export type FanMode = 'auto' | 'max' | 'custom';
export interface ThermalPoint { celsius: number; percent: number }
export interface ThermalCurve { points: ThermalPoint[]; hysteresis_celsius: number; minimum_write_interval_ms: number; maximum_sample_age_ms: number }
export interface ThermalConfig { cpu: ThermalCurve; gpu: ThermalCurve }
export interface ThermalStatus { active: boolean; restore_pending: boolean; message: string; cpu_target: number|null; gpu_target: number|null; last_report: unknown }
export type Theme = 'system' | 'light' | 'dark';
export type CapabilityStatus = 'verified' | 'implemented_unverified' | 'protocol_pending' | 'permission_blocked' | 'hardware_unsupported';
export interface Capability { supported: boolean; reason?: string | null; diagnostic_reason?: string | null; status?: CapabilityStatus }
export interface Capabilities {
  model: string;
  bios_version: string | null;
  telemetry: Capability;
  profiles: Profile[];
  profile_control: Capability;
  fan_control: Capability;
  fan_custom_control: Capability;
  fan_percent_range: { min: number; max: number } | null;
  lighting_control: Capability;
  lighting_dynamic_control: Capability;
  lighting_restore_control: Capability;
  battery_control: Capability;
}
export interface Snapshot {
  timestamp: string;
  cpu_temperature: number | null;
  gpu_temperature: number | null;
  cpu_fan_rpm: number | null;
  gpu_fan_rpm: number | null;
  profile: Profile | null;
  fan_mode: FanMode | null;
  cpu_fan_percent: number | null;
  gpu_fan_percent: number | null;
  ac_power: boolean | null;
  battery_percent: number | null;
  battery_care: boolean | null;
  lighting: Lighting | null;
  lighting_config: LightingConfig | null;
  errors: string[];
}
export interface Lighting { enabled: boolean; brightness: number; zones?: string[] | null; color?: string }
export type LightingEffect = 'static' | 'breathing' | 'neon' | 'wave' | 'shifting' | 'zoom' | 'meteor' | 'twinkling';
export type DynamicLightingEffect = Exclude<LightingEffect, 'static'>;
export interface LightingConfig { effect: LightingEffect; speed: number; brightness: number; direction: 1 | 2; color: string | null; zones: string[] | null }
export interface LightingAnimation { effect: DynamicLightingEffect; speed: number; brightness: number; direction: 1 | 2; color: string }
export interface StartupState { enabled: boolean; entry_present: boolean; mismatch: boolean }
export type OfficialTaskAction = 'query' | 'backup' | 'disable' | 'restore';
export interface OfficialTaskState {
  task_name: string;
  enabled: boolean;
  definition_sha256: string;
  backup_valid: boolean;
  backup_original_enabled: boolean | null;
  backup_error: string | null;
}
export interface RulesConfig { enabled: boolean; ac_profile: Profile | null; dc_profile: Profile | null }
export interface RulesState { config: RulesConfig; status: string; ac_profiles: Profile[]; dc_profiles: Profile[] }
export type VendorSetting = 'lcd_overdrive' | 'boot_sound' | 'sticky_key' | 'backlight_timeout';
export interface VendorControlItem { setting: VendorSetting; enabled: boolean | null; writable: boolean; error: string | null; raw: unknown }
export interface VendorControlsState { backend: 'official_service' | 'mixed'; dependency: string; items: VendorControlItem[] }
export type UsbChargeThreshold = 10 | 20 | 30;
export interface UsbChargeState { enabled: boolean; threshold: UsbChargeThreshold }
export interface UsbChargeReadback { state: UsbChargeState | null; writable: boolean; ack: 'confirmed' | 'rejected' | 'unknown'; error: string | null; backend: 'direct_wmi'; raw: unknown }
export interface UsbChargeResult extends OperationResult { state: UsbChargeState | null }
export type AudioPreset = 'music' | 'movie' | 'voice' | 'strategy' | 'rpg' | 'shooter' | 'custom' | 'automatic';
export interface AudioState { backend: 'official_service'; dependency: string; supported: AudioPreset[]; preset: AudioPreset | null; availability: 'available' | 'output_unavailable' | 'unknown'; writable: boolean; error: string | null }
export type AudioResult = OperationResult;
export interface LogoEntry { slot: string; present: boolean; sha256: string | null }
export interface LogoManifest { entries: LogoEntry[] }
export interface BootLogoState {
  inspection: { discovery: { model: string; bios: string; maximum_image_pixels: [number, number] }; states: LogoManifest };
  backups: { id: string; state: string }[];
  pending?: LogoPlan | null;
}
export interface LogoPlan { selected_file?: string | null; plan_id: string; backup_id: string; review: { targets: string[]; before: LogoManifest; after: LogoManifest; backup: string }; expires_seconds: number }
export interface LogoResult extends OperationResult { report?: unknown }
export interface ExitStatus {
  error: string | null;
  pending: {
    enabled: boolean;
    has_receipt: boolean;
    restore_command: unknown;
    blocked_reason: string | null;
    allow_exit: boolean;
    in_progress: boolean;
  };
}
export interface Settings {
  theme: Theme;
  poll_interval_ms: number;
  close_to_tray: boolean;
  restore_on_exit: boolean;
}
export interface OperationResult {
  status: 'applied' | 'failed' | 'unconfirmed';
  message: string;
}

async function command<T>(name: string, args?: Record<string, unknown>): Promise<T> {
  if (!isTauri()) throw new Error('当前为浏览器预览。请在桌面应用中连接设备。');
  return invoke<T>(name, args);
}
export const api = {
  getThermalConfig: () => command<ThermalConfig|null>('get_thermal_config'),
  saveThermalConfig: (config: ThermalConfig) => command<void>('save_thermal_config',{config}),
  getThermalStatus: () => command<ThermalStatus>('get_thermal_status'),
  startThermal: (config: ThermalConfig) => command<ThermalStatus>('start_thermal',{config}),
  stopThermal: () => command<ThermalStatus>('stop_thermal'),
  getDeviceCenter: () => command<DeviceCenterState>('get_device_center_plugin'),
  bindDeviceCenter: () => command<DeviceCenterState|null>('bind_device_center_plugin'),
  bindDeviceCenterModule: () => command<DeviceCenterState|null>('bind_device_center_module'),
  deviceCenterAction: (action: DeviceCenterAction) => command<{message:string;log_error?:string}>('device_center_action',{action}),
  getDeviceCenterLog: () => command<{text:string;truncated:boolean}>('get_device_center_management_log'),
  getCentralPlugin: () => command<CentralPluginState>('get_central_plugin'),
  setCentralGuard: (action: 'enable' | 'disable') => command<CentralPluginState>('set_central_guard', { action }),
  getCentralLog: (log: PluginLog) => command<PluginLogResult>('get_central_log', { log }),
  getSnapshot: () => command<Snapshot>('get_snapshot'),
  getCapabilities: () => command<Capabilities>('get_capabilities'),
  // Explicit user action only: may open Windows UAC. Never call from polling/retries.
  authorizeDevice: () => command<void>('authorize_device'),
  // Explicit fallback only after an exit-restore-error; never called automatically.
  exitWithoutRestore: () => command<void>('exit_without_restore'),
  getExitStatus: () => command<ExitStatus>('get_exit_status'),
  setProfile: (profile: Profile) => command<OperationResult>('set_profile', { profile }),
  setFanMode: (mode: Exclude<FanMode, 'custom'>) => command<OperationResult>('set_fan_mode', { mode }),
  setFanSpeeds: (cpuPercent: number, gpuPercent: number) => command<OperationResult>('set_fan_speeds', { cpuPercent, gpuPercent }),
  setLighting: (lighting: Lighting) => command<OperationResult>('set_lighting', { lighting }),
  setLightingAnimation: (lighting: LightingAnimation) => command<OperationResult>('set_lighting_animation', { lighting }),
  restoreLighting: () => command<OperationResult>('restore_lighting'),
  setBatteryCare: (enabled: boolean) => command<OperationResult>('set_battery_care', { enabled }),
  getSettings: () => command<Settings>('get_settings'),
  previewTheme: (theme: Theme) => command<void>('preview_theme', { theme }),
  saveSettings: (settings: Settings) => command<Settings>('save_settings', { settings }),
  getStartup: () => command<StartupState>('get_startup'),
  setStartup: (enabled: boolean) => command<StartupState>('set_startup', { enabled }),
  getRules: () => command<RulesState>('get_rules'),
  saveRules: (config: RulesConfig) => command<void>('save_rules', { config }),
  officialTask: (action: OfficialTaskAction) => command<OfficialTaskState>('official_task', { action }),
  getVendorControls: () => command<VendorControlsState>('get_vendor_controls'),
  setVendorControl: (setting: VendorSetting, enabled: boolean) => command<OperationResult>('set_vendor_control', { setting, enabled }),
  getUsbCharge: () => command<UsbChargeReadback>('get_usb_charge'),
  setUsbCharge: (state: UsbChargeState) => command<UsbChargeResult>('set_usb_charge', { state }),
  getAudio: () => command<AudioState>('get_audio'),
  setAudio: (preset: AudioPreset) => command<AudioResult>('set_audio', { preset }),
  getBootLogo: () => command<BootLogoState>('get_boot_logo'),
  chooseBootLogo: () => command<LogoPlan | null>('choose_boot_logo'),
  prepareBootLogoReset: () => command<LogoPlan>('prepare_boot_logo_reset'),
  prepareBootLogoRestore: (backupId: string) => command<LogoPlan>('prepare_boot_logo_restore', { backupId }),
  commitBootLogo: (planId: string) => command<LogoResult>('commit_boot_logo', { planId }),
  discardBootLogo: (planId: string) => command<void>('discard_boot_logo', { planId }),
};
export type PluginLog = 'events' | 'previous_events' | 'last_check' | 'activation';
export type DeviceCenterAction = 'launch' | 'enable_startup' | 'disable_startup' | 'detach';
export interface DeviceCenterModuleInfo {
  root: string;
  id: string;
  name: string;
  version: string;
}
export interface DeviceCenterState {
  configured: boolean; executable: string | null; module?: DeviceCenterModuleInfo | null; binding_valid: boolean;binding_error:string|null;captured_at_ms:number;logical_processors:number;running:boolean;
  processes:{pid:number;name:string;created:string|null;cpu_seconds:number|null;working_set_bytes:number;private_commit_bytes:number;handles:number;threads:number}[];
  startup:{present:boolean;enabled:boolean;mismatch:boolean;other_entries:string[]};log_error?:string|null;
}
export interface PluginLogResult { text: string; truncated: boolean; file_bytes: number }
export interface CentralMetrics { pid: number; started_at: string; cpu_seconds: number; working_set_bytes: number; private_commit_bytes: number; handles: number; threads: number }
export interface CentralPluginState {
  captured_at_ms: number;
  logical_processors: number;
  service: { state: string; start_mode: string; pid: number; path: string; patch: 'patched' | 'original' | 'unknown'; sha256: string | null } | null;
  metrics: CentralMetrics | null;
  guard: { installed: boolean | null; enabled: boolean | null; state: number | null; verified: boolean; last_run: string | null; next_run: string | null; last_result: number | null; threshold: number; cooldown_seconds: number; error: string | null };
  errors: string[];
}
export const errorText = (error: unknown): string => {
  if (error instanceof Error) return error.message;
  if (typeof error === 'string') return error;
  try { return JSON.stringify(error); } catch { return '请求失败，请重试。'; }
};
