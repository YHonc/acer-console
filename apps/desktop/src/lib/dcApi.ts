import { invoke } from '@tauri-apps/api/core';
import type { DcCallResult, DcError, DcMethod, DcModuleStatus } from './dcTypes';
export class DcCallError extends Error {
  constructor(public fault: DcError) { super(fault.code === 'TIMEOUT' || fault.code === 'PARTIAL_WRITE' || (fault.details as {reason?:string}|null)?.reason === 'disconnected' ? '结果未知，请刷新读回' : fault.message); }
}
export const dcApi = {
  status: () => invoke<DcModuleStatus>('dc_module_status'),
  start: () => invoke<DcModuleStatus>('dc_module_trust_and_start'),
  keep: (keep: boolean) => invoke<DcModuleStatus>('dc_module_set_keep', {keep}),
  async call<T>(method: DcMethod, params: object = {}): Promise<T> {
    const response = await invoke<DcCallResult<T>>('dc_call', {method,params});
    if (!response.ok) throw new DcCallError(response.error);
    return response.result;
  }
};
