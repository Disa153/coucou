import test from "node:test";
import assert from "node:assert/strict";
import { batteryAlerts, DEMO_BATTERIES } from "../src/core/battery.ts";

const device = (overrides = {}) => ({
  id: "mouse", name: "Mouse", kind: "mouse", percent: 20,
  charging: false, source: "hid", ...overrides,
});

test("shows only reported low non-charging batteries", () => {
  const alerts = batteryAlerts([
    device(), device({ id: "full", percent: 100 }),
    device({ id: "charging", charging: true }),
    device({ id: "unknown", percent: null }),
    device({ id: "unconfirmed", charging: null }),
  ]);
  assert.deepEqual(alerts.map(a => a.id), ["mouse"]);
  assert.equal(alerts[0].severity, "low");
});

test("critical batteries come first and invalid percentages are ignored", () => {
  const alerts = batteryAlerts([
    device({ id: "low", percent: 30 }),
    device({ id: "critical", percent: 15 }),
    device({ id: "negative", percent: -1 }),
    device({ id: "invalid", percent: NaN }),
  ]);
  assert.deepEqual(alerts.map(a => [a.id, a.severity]), [
    ["critical", "critical"], ["low", "low"],
  ]);
});

test("demo samples never masquerade as measured battery alerts", () => {
  assert.deepEqual(batteryAlerts(DEMO_BATTERIES), []);
});
