import { batteryAlerts, type BatteryAlert, type BatteryDevice } from "./battery";

/** Runtime battery feed. Empty until a trusted Windows/HID reader is connected. */
let devices: readonly BatteryDevice[] = [];
const listeners = new Set<() => void>();

export function setBatteryDevices(next: readonly BatteryDevice[]): void {
  devices = next.filter(d => d.source !== "demo").map(d => ({ ...d }));
  for (const listener of listeners) listener();
}

export function getBatteryAlerts(): BatteryAlert[] {
  return batteryAlerts(devices);
}

export function subscribeBatteryDevices(listener: () => void): () => void {
  listeners.add(listener);
  return () => { listeners.delete(listener); };
}
