import { readdir, readFile } from "node:fs/promises";
import path from "node:path";

const ROOT = process.cwd();
const MAX_LINES = 1000;
const TARGET_DIRS = ["src", path.join("src-tauri", "src")];
const SOURCE_EXTENSIONS = new Set([".rs", ".ts", ".tsx", ".css"]);

const gatherFiles = async (directoryPath) => {
  const entries = await readdir(directoryPath, { withFileTypes: true });
  const files = [];
  for (const entry of entries) {
    const fullPath = path.join(directoryPath, entry.name);
    if (entry.isDirectory()) {
      files.push(...(await gatherFiles(fullPath)));
      continue;
    }
    if (entry.isFile() && SOURCE_EXTENSIONS.has(path.extname(entry.name))) {
      files.push(fullPath);
    }
  }
  return files;
};

const lineCount = async (filePath) => {
  const content = await readFile(filePath, "utf8");
  if (content.length === 0) {
    return 0;
  }
  return content.split(/\r?\n/).length;
};

const run = async () => {
  const filePaths = [];
  for (const relativeDirectory of TARGET_DIRS) {
    const fullDirectory = path.join(ROOT, relativeDirectory);
    filePaths.push(...(await gatherFiles(fullDirectory)));
  }

  const oversized = [];
  for (const filePath of filePaths) {
    const count = await lineCount(filePath);
    if (count > MAX_LINES) {
      oversized.push({
        relativePath: path.relative(ROOT, filePath),
        count,
      });
    }
  }

  if (oversized.length > 0) {
    oversized.sort((a, b) => b.count - a.count);
    console.error(`Line cap violation: files must be <= ${MAX_LINES} lines.`);
    for (const violation of oversized) {
      console.error(`- ${violation.relativePath}: ${violation.count}`);
    }
    process.exitCode = 1;
    return;
  }

  console.log(`Line cap check passed (${MAX_LINES} max lines).`);
};

await run();
