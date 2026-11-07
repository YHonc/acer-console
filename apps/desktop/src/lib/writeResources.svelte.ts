// Shared write ownership. Reads never acquire a resource.
// Performance/fans/thermal and lighting share the device controller.
type Resource = 'performance' | 'lighting' | 'battery' | 'settings' | 'startup' | 'rules' | 'auth' | 'exit' | 'audio' | 'usb' | 'official' | 'central' | 'deviceCenter' | `vendor:${string}` | `dc:${string}`;
let active = $state<Resource[]>([]);
export function anyWriting() { return active.length > 0; }
export function isWriting(resource: Resource) { return active.includes(resource); }
export function writeBlocked(resource: Resource) {
  return active.some(other => other === resource || other === 'auth' || other === 'exit' ||
    resource === 'auth' || resource === 'exit' ||
    ((resource === 'performance' || resource === 'lighting') && (other === 'performance' || other === 'lighting')));
}
export function beginWrite(resource: Resource): (() => void) | null {
  if (writeBlocked(resource)) return null;
  active = [...active, resource];
  return () => { active = active.filter(item => item !== resource); };
}
