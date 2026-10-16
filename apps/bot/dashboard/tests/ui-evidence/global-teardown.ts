import { writeManifest } from "./support/evidence";

/** Fold the per-capture records into one SHA-bound evidence manifest. */
export default async function globalTeardown() {
  const dest = writeManifest();
  // Surfaced in the workflow log so the run itself names its manifest.
  console.log(`[ui-evidence] manifest written: ${dest}`);
}
