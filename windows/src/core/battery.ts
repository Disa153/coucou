/** Nook battery presentation model. No hardware values are inferred. */
export type BatteryDeviceKind = "headset" | "keyboard" | "mouse" | "other";
export type BatterySource = "windows" | "hid" | "demo";
export interface BatteryDevice {
  id: string;
  name: string;
  kind: BatteryDeviceKind;
  /** null means the device/protocol did not report a percentage. */
  percent: number | null;
  charging: boolean | null;
  source: BatterySource;
}
export interface BatteryAlert extends BatteryDevice {
  severity: "low" | "critical";
}
export const LOW_BATTERY_PERCENT = 30;
export const CRITICAL_BATTERY_PERCENT = 15;

/** Only report trustworthy, non-charging low batteries. */
export function batteryAlerts(devices: readonly BatteryDevice[]): BatteryAlert[] {
  return devices
    .filter((d): d is BatteryDevice & { percent: number } =>
      d.source !== "demo" &&
      d.charging === false &&
      d.percent !== null &&
      Number.isFinite(d.percent) &&
      d.percent >= 0 &&
      d.percent <= LOW_BATTERY_PERCENT)
    .map(d => ({
      ...d,
      severity: d.percent <= CRITICAL_BATTERY_PERCENT ? "critical" as const : "low" as const,
    }))
    .sort((a, b) => a.percent - b.percent);
}

/** Demo data is intentionally separate and never enters real alert output. */
export const DEMO_BATTERIES: readonly BatteryDevice[] = [
  { id: "demo-g435", name: "Logitech G435", kind: "headset", percent: 22, charging: false, source: "demo" },
  { id: "demo-f75", name: "AULA F75", kind: "keyboard", percent: 78, charging: false, source: "demo" },
  { id: "demo-mouse", name: "Wireless mouse", kind: "mouse", percent: null, charging: null, source: "demo" },
];
