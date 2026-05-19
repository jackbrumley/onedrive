import { readdir, readFile } from "node:fs/promises";
import path from "node:path";

const repoRoot = process.cwd();
const appRoot = path.join(repoRoot, "src-tauri", "src", "app");

const allowedFiles = new Set([
  path.join(appRoot, "sync_runtime.rs"),
  path.join(appRoot, "sync_engine", "lifecycle_writer.rs"),
  path.join(appRoot, "sync_engine", "runtime_and_models.rs"),
  path.join(appRoot, "commands", "accounts", "profile_commands.rs"),
]);

const forbiddenPattern = /sync_runtime::(?:set_|clear_|record_|start_transfer|update_transfer_progress|finish_transfer|remove_account)/g;

const lifecycleAllowedFiles = new Set([
  path.join(appRoot, "sync_engine", "lifecycle_writer.rs"),
  path.join(appRoot, "sync_engine", "job_queue_lifecycle_store.rs"),
]);

const forbiddenLifecycleWritePattern = /persist_sync_lifecycle_(?:phase|activity|remote_scan_complete)\(/g;

async function collectRustFiles(dir) {
  const entries = await readdir(dir, { withFileTypes: true });
  const files = [];
  for (const entry of entries) {
    const absolute = path.join(dir, entry.name);
    if (entry.isDirectory()) {
      files.push(...(await collectRustFiles(absolute)));
      continue;
    }
    if (entry.isFile() && entry.name.endsWith(".rs")) {
      files.push(absolute);
    }
  }
  return files;
}

async function main() {
  const rustFiles = await collectRustFiles(appRoot);
  const violations = [];

  for (const rustFile of rustFiles) {
    if (allowedFiles.has(rustFile)) {
      continue;
    }
    const content = await readFile(rustFile, "utf8");
    const match = content.match(forbiddenPattern);
    if (match && match.length > 0) {
      violations.push(path.relative(repoRoot, rustFile));
    }
  }

  const lifecycleViolations = [];
  for (const rustFile of rustFiles) {
    if (lifecycleAllowedFiles.has(rustFile) || rustFile.endsWith("_tests.rs")) {
      continue;
    }
    const content = await readFile(rustFile, "utf8");
    const match = content.match(forbiddenLifecycleWritePattern);
    if (match && match.length > 0) {
      lifecycleViolations.push(path.relative(repoRoot, rustFile));
    }
  }

  if (violations.length > 0) {
    console.error("Forbidden sync_runtime write calls found outside central writer files:");
    for (const violation of violations) {
      console.error(` - ${violation}`);
    }
    process.exit(1);
  }

  if (lifecycleViolations.length > 0) {
    console.error("Forbidden lifecycle direct-write calls found outside lifecycle writer/store:");
    for (const violation of lifecycleViolations) {
      console.error(` - ${violation}`);
    }
    process.exit(1);
  }

  console.log("Sync status writer guard passed.");
}

main().catch((error) => {
  console.error(`Failed sync status writer guard: ${String(error)}`);
  process.exit(1);
});
