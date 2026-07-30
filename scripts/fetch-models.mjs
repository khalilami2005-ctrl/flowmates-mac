import { spawnSync } from "node:child_process";
import { createHash } from "node:crypto";
import {
  createReadStream,
  createWriteStream,
  existsSync,
  mkdirSync,
  readdirSync,
  renameSync,
  statSync,
  unlinkSync,
} from "node:fs";
import { dirname, join } from "node:path";
import { Readable, Transform } from "node:stream";
import { pipeline } from "node:stream/promises";
import { setTimeout as delay } from "node:timers/promises";
import { fileURLToPath } from "node:url";

// Where the weights are hosted. Nothing is downloaded while the files already
// on disk match the sizes and digests below, so a checkout that ships them
// never touches this.
const DEFAULT_REPO = "khalilami2005-ctrl/flowmates-releases";
const DEFAULT_TAG = "models-v1";
const DOWNLOAD_TIMEOUT_MS = 30 * 60 * 1_000;
const MAX_ATTEMPTS = 3;

const LLAMA_SERVER = {
  path: "local_llm/bin/llama-server",
  size: 32_597_376,
  sha256: "91daa04508cd9159642debaf36cfefc7c5ee4c6ef9405bdbfcebdf38b3d0c2f6",
};

const ASSETS = {
  "Qwen3-VL-2B-Instruct-Q3_K_M.gguf": {
    path: "local_llm/Qwen3-VL-2B-Instruct-Q3_K_M.gguf",
    size: 939_540_160,
    sha256: "d4346b52a40d103ed6892b09fd3643e0a11b2dd26d3234f37ec68a94ec20ae24",
  },
  "mmproj-Qwen3VL-2B-Instruct-Q8_0.gguf": {
    path: "local_llm/mmproj-Qwen3VL-2B-Instruct-Q8_0.gguf",
    size: 445_053_216,
    sha256: "f9a68fabba69c3b81e153367b2c7521030b0fa8bb0de400c9599c8e6725f9c82",
  },
};

const scriptDir = dirname(fileURLToPath(import.meta.url));

function repoRoot() {
  let dir = join(scriptDir, "..");
  for (let depth = 0; depth < 10; depth += 1) {
    if (existsSync(join(dir, ".git"))) return dir;
    const parent = dirname(dir);
    if (parent === dir) break;
    dir = parent;
  }
  return join(scriptDir, "..");
}

function resolveGithubToken() {
  for (const key of ["GITHUB_TOKEN", "GH_TOKEN"]) {
    const value = process.env[key]?.trim();
    if (value) return value;
  }

  try {
    const result = spawnSync("gh", ["auth", "token"], {
      encoding: "utf8",
      timeout: 10_000,
    });
    if (result.status === 0 && result.stdout?.trim()) return result.stdout.trim();
  } catch {
    return null;
  }
  return null;
}

function humanSize(bytes) {
  let value = Number(bytes);
  let unit = 0;
  const units = ["B", "KiB", "MiB", "GiB"];
  while (value >= 1024 && unit < units.length - 1) {
    value /= 1024;
    unit += 1;
  }
  return `${value.toFixed(1)} ${units[unit]}`;
}

function assetUrl(repo, tag, name) {
  return `https://github.com/${repo}/releases/download/${tag}/${name}`;
}

async function sha256File(path) {
  const hash = createHash("sha256");
  for await (const chunk of createReadStream(path)) hash.update(chunk);
  return hash.digest("hex");
}

async function verifyFile(path, metadata) {
  if (!existsSync(path)) return { ok: false, reason: "missing" };

  let size;
  try {
    size = statSync(path).size;
  } catch (error) {
    return { ok: false, reason: `cannot stat: ${error.message}` };
  }

  if (size !== metadata.size) {
    return {
      ok: false,
      reason: `size ${size}, expected ${metadata.size}`,
    };
  }

  const digest = await sha256File(path);
  if (digest !== metadata.sha256) {
    return {
      ok: false,
      reason: `SHA-256 ${digest}, expected ${metadata.sha256}`,
    };
  }

  return { ok: true };
}

async function verifyLlamaServer(root) {
  const path = join(root, LLAMA_SERVER.path);
  const binDir = dirname(path);
  if (!existsSync(binDir)) return { ok: false, reason: "runtime directory is missing" };
  const forbidden = readdirSync(binDir, { withFileTypes: true })
    .filter((entry) => entry.isFile() && /\.(?:exe|dll)$/i.test(entry.name))
    .map((entry) => entry.name);
  if (forbidden.length > 0) {
    return {
      ok: false,
      reason: `Windows runtime artifacts are forbidden: ${forbidden.join(", ")}`,
    };
  }

  const status = await verifyFile(path, LLAMA_SERVER);
  if (!status.ok) return status;
  if ((statSync(path).mode & 0o111) === 0) {
    return { ok: false, reason: "file is not executable" };
  }

  const lipo = spawnSync("lipo", [path, "-verify_arch", "arm64", "x86_64"], {
    encoding: "utf8",
    timeout: 10_000,
  });
  if (lipo.status !== 0) {
    const detail = lipo.stderr?.trim() || lipo.error?.message || "lipo failed";
    return { ok: false, reason: `not a universal arm64+x86_64 Mach-O: ${detail}` };
  }

  return { ok: true };
}

function removePart(path) {
  try {
    unlinkSync(path);
  } catch (error) {
    if (error.code !== "ENOENT") throw error;
  }
}

function httpError(response) {
  const error = new Error(`HTTP ${response.status} ${response.statusText}`);
  error.httpCode = response.status;
  return error;
}

function retryable(error) {
  const code = error.httpCode;
  return (
    code === undefined ||
    code === 408 ||
    code === 429 ||
    code >= 500
  );
}

async function downloadOnce(url, dest, metadata, token) {
  mkdirSync(dirname(dest), { recursive: true });
  const part = `${dest}.part`;
  removePart(part);

  const headers = {
    Accept: "application/octet-stream",
    "User-Agent": "flowmates-fetch-models",
  };
  if (token) headers.Authorization = `Bearer ${token}`;

  try {
    const response = await fetch(url, {
      redirect: "follow",
      headers,
      signal: AbortSignal.timeout(DOWNLOAD_TIMEOUT_MS),
    });
    if (!response.ok) throw httpError(response);
    if (!response.body) throw new Error("Empty response body");

    const contentLength = Number(response.headers.get("content-length") || 0);
    if (contentLength > 0 && contentLength !== metadata.size) {
      throw new Error(
        `Unexpected Content-Length ${contentLength}; expected ${metadata.size}`,
      );
    }

    const hash = createHash("sha256");
    let downloaded = 0;
    let lastProgressAt = 0;
    const meter = new Transform({
      transform(chunk, _encoding, callback) {
        downloaded += chunk.length;
        hash.update(chunk);

        const now = Date.now();
        if (now - lastProgressAt >= 250) {
          const percent = (downloaded * 100) / metadata.size;
          process.stdout.write(
            `  ${humanSize(downloaded)} / ${humanSize(metadata.size)} (${percent.toFixed(1)}%)\r`,
          );
          lastProgressAt = now;
        }
        callback(null, chunk);
      },
    });

    await pipeline(
      Readable.fromWeb(response.body),
      meter,
      createWriteStream(part, { flags: "wx" }),
    );
    process.stdout.write("\n");

    const digest = hash.digest("hex");
    if (downloaded !== metadata.size) {
      throw new Error(`Downloaded ${downloaded} bytes; expected ${metadata.size}`);
    }
    if (digest !== metadata.sha256) {
      throw new Error(`SHA-256 ${digest}; expected ${metadata.sha256}`);
    }

    renameSync(part, dest);
  } catch (error) {
    removePart(part);
    throw error;
  }
}

async function download(url, dest, metadata, token) {
  let lastError;
  for (let attempt = 1; attempt <= MAX_ATTEMPTS; attempt += 1) {
    try {
      await downloadOnce(url, dest, metadata, token);
      return;
    } catch (error) {
      lastError = error;
      if (!retryable(error) || attempt === MAX_ATTEMPTS) break;
      const waitMs = 2 ** (attempt - 1) * 2_000;
      console.warn(
        `  attempt ${attempt}/${MAX_ATTEMPTS} failed: ${error.message}; retrying in ${waitMs / 1_000}s`,
      );
      await delay(waitMs);
    }
  }
  throw lastError;
}

async function main() {
  const force = process.argv.includes("--force");
  const check = process.argv.includes("--check");
  const checkBinaryOnly = process.argv.includes("--check-binary");
  const repo = process.env.FLOWMATES_MODELS_REPO || DEFAULT_REPO;
  const tag = process.env.FLOWMATES_MODELS_TAG || DEFAULT_TAG;
  const root = repoRoot();

  console.log(`[fetch-models] repo=${repo} tag=${tag}`);
  console.log(`[fetch-models] root=${root}`);

  const binaryStatus = await verifyLlamaServer(root);
  if (!binaryStatus.ok) {
    console.error(`[fetch-models] INVALID ${LLAMA_SERVER.path}: ${binaryStatus.reason}`);
    console.error("  A trusted universal macOS llama-server must be present in the repository.");
    return check || checkBinaryOnly ? 1 : 2;
  }
  console.log(
    `  OK    ${LLAMA_SERVER.path} (${humanSize(LLAMA_SERVER.size)}, SHA-256 and arm64+x86_64 verified)`,
  );
  if (checkBinaryOnly) return 0;

  const pending = [];
  for (const [name, metadata] of Object.entries(ASSETS)) {
    const dest = join(root, metadata.path);
    if (force) {
      pending.push({ name, dest, metadata, reason: "forced" });
      continue;
    }

    const status = await verifyFile(dest, metadata);
    if (status.ok) {
      console.log(`  OK    ${metadata.path} (${humanSize(metadata.size)}, SHA-256 verified)`);
    } else {
      pending.push({ name, dest, metadata, reason: status.reason });
    }
  }

  if (pending.length === 0) {
    console.log("[fetch-models] all assets verified.");
    return 0;
  }

  if (check) {
    console.error("[fetch-models] INVALID (--check):");
    for (const item of pending) {
      console.error(`  -- ${item.metadata.path}: ${item.reason}`);
    }
    return 1;
  }

  const token = resolveGithubToken();
  if (token) console.log("[fetch-models] using auth token from env/gh");

  for (const item of pending) {
    const url = assetUrl(repo, tag, item.name);
    console.log(`[fetch-models] downloading ${item.name} (${item.reason})`);
    console.log(`  from ${url}`);
    console.log(`  to   ${item.dest}`);

    try {
      await download(url, item.dest, item.metadata, token);
      console.log(`  OK   SHA-256 ${item.metadata.sha256}`);
    } catch (error) {
      const code = error.httpCode;
      if (code === 404) {
        console.error(`  Release '${tag}' or asset '${item.name}' does not exist in ${repo}.`);
      } else if (code === 401 || code === 403) {
        console.error("  Authentication failed. Set GITHUB_TOKEN or run `gh auth login`.");
      }
      console.error(`  ERROR: ${error.message}`);
      return 2;
    }
  }

  console.log("[fetch-models] done.");
  return 0;
}

main().then(
  (code) => process.exit(code),
  (error) => {
    console.error(error);
    process.exit(2);
  },
);
