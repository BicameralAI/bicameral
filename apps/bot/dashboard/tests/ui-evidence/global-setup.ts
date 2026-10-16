import fs from "node:fs";
import { evidenceRoot } from "./support/evidence";

/** Start every run from an empty evidence tree so nothing stale is republished. */
export default async function globalSetup() {
  fs.rmSync(evidenceRoot(), { recursive: true, force: true });
}
