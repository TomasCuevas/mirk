// r12 spike: localhost static server + result collector.
// Serves the measuring page over HTTP (never file://, ES modules + workers need a real origin)
// and accepts the self-measured JSON results POSTed by the page.
//
// Static mounts:
//   /          -> ./public
//   /vendor/   -> ./vendor/pdfjs-dist   (pdf.js ESM build + worker + cmaps + wasm)
//
// Endpoints:
//   POST /result  -> append JSON body to results.raw.jsonl, write logs/result-<run>.json
//   POST /log     -> append text lines to logs/page.log
//   GET  /health  -> { ok: true }
//   GET  /results -> all collected raw results
import http from "node:http";
import fs from "node:fs";
import fsp from "node:fs/promises";
import path from "node:path";
import url from "node:url";

const HERE = path.dirname(url.fileURLToPath(import.meta.url));
const PUBLIC_DIR = path.join(HERE, "public");
const VENDOR_DIR = path.join(HERE, "vendor", "pdfjs-dist");
const LOGS_DIR = path.join(HERE, "logs");
const RAW_RESULTS = path.join(HERE, "results.raw.jsonl");
const PORT = Number(process.env.PORT || 8123);

fs.mkdirSync(LOGS_DIR, { recursive: true });

// per-file transfer stats: evidence for whether pdf.js streamed the PDF with Range requests
const stats = { startedAt: new Date().toISOString(), files: {} };
function noteStat(abs, ranged, bytes) {
  const key = path.relative(HERE, abs);
  const s = (stats.files[key] ||= { requests: 0, rangedRequests: 0, bytesServed: 0 });
  s.requests++;
  if (ranged) s.rangedRequests++;
  s.bytesServed += bytes;
}

const MIME = {
  ".html": "text/html; charset=utf-8",
  ".js": "text/javascript; charset=utf-8",
  ".mjs": "text/javascript; charset=utf-8",
  ".css": "text/css; charset=utf-8",
  ".json": "application/json; charset=utf-8",
  ".pdf": "application/pdf",
  ".wasm": "application/wasm",
  ".bcmap": "application/octet-stream",
  ".pfb": "application/octet-stream",
  ".ttf": "font/ttf",
  ".icc": "application/octet-stream",
  ".map": "application/json; charset=utf-8",
  ".svg": "image/svg+xml",
  ".png": "image/png",
};

// map a URL pathname to an absolute file path, refusing anything outside the mounts
function resolveFile(pathname) {
  const decoded = decodeURIComponent(pathname.split("?")[0]);
  let mount, rel;
  if (decoded === "/" || decoded === "/index.html") return path.join(PUBLIC_DIR, "index.html");
  if (decoded.startsWith("/vendor/")) {
    mount = VENDOR_DIR;
    rel = decoded.slice("/vendor/".length);
  } else {
    mount = PUBLIC_DIR;
    rel = decoded.slice(1);
  }
  const abs = path.resolve(mount, rel);
  if (abs !== mount && !abs.startsWith(mount + path.sep)) return null;
  return abs;
}

function sendJson(res, code, obj) {
  const body = Buffer.from(JSON.stringify(obj));
  res.writeHead(code, {
    "content-type": "application/json; charset=utf-8",
    "content-length": body.length,
    "cache-control": "no-store",
  });
  res.end(body);
}

function readBody(req, limit = 8 * 1024 * 1024) {
  return new Promise((resolve, reject) => {
    const chunks = [];
    let size = 0;
    req.on("data", (c) => {
      size += c.length;
      if (size > limit) {
        reject(new Error("body too large"));
        req.destroy();
        return;
      }
      chunks.push(c);
    });
    req.on("end", () => resolve(Buffer.concat(chunks)));
    req.on("error", reject);
  });
}

// PDF delivery needs Range support: pdf.js range-fetches large files instead of
// downloading 136 MB whole, and so does Chrome's PDF stack.
async function serveFile(req, res, abs) {
  let st;
  try {
    st = await fsp.stat(abs);
  } catch {
    return sendJson(res, 404, { error: "not found", path: abs });
  }
  if (st.isDirectory()) return sendJson(res, 404, { error: "is a directory" });
  // follow symlinks for the real size (fs.stat already does)

  const type = MIME[path.extname(abs).toLowerCase()] || "application/octet-stream";
  const range = req.headers.range;
  const common = {
    "content-type": type,
    "accept-ranges": "bytes",
    "cache-control": "no-store",
  };

  if (range) {
    const m = /^bytes=(\d*)-(\d*)$/.exec(range.trim());
    if (!m) {
      res.writeHead(416, { ...common, "content-range": `bytes */${st.size}` });
      return res.end();
    }
    let start = m[1] === "" ? null : Number(m[1]);
    let end = m[2] === "" ? null : Number(m[2]);
    if (start === null && end === null) {
      res.writeHead(416, { ...common, "content-range": `bytes */${st.size}` });
      return res.end();
    }
    if (start === null) {
      start = Math.max(0, st.size - end);
      end = st.size - 1;
    } else if (end === null || end >= st.size) {
      end = st.size - 1;
    }
    if (start > end || start >= st.size) {
      res.writeHead(416, { ...common, "content-range": `bytes */${st.size}` });
      return res.end();
    }
    res.writeHead(206, {
      ...common,
      "content-range": `bytes ${start}-${end}/${st.size}`,
      "content-length": end - start + 1,
    });
    noteStat(abs, true, end - start + 1);
    if (req.method === "HEAD") return res.end();
    return fs.createReadStream(abs, { start, end }).pipe(res);
  }

  res.writeHead(200, { ...common, "content-length": st.size });
  noteStat(abs, false, st.size);
  if (req.method === "HEAD") return res.end();
  return fs.createReadStream(abs).pipe(res);
}

const server = http.createServer(async (req, res) => {
  const u = new URL(req.url, "http://localhost");
  try {
    if (req.method === "POST" && u.pathname === "/result") {
      const body = await readBody(req);
      let parsed = null;
      try {
        parsed = JSON.parse(body.toString("utf8"));
      } catch (e) {
        return sendJson(res, 400, { error: "invalid json", message: String(e) });
      }
      const line = JSON.stringify(parsed);
      await fsp.appendFile(RAW_RESULTS, line + "\n");
      const tag = `${parsed.engine || "unknown"}-${parsed.run || "norun"}`;
      await fsp.writeFile(path.join(LOGS_DIR, `result-${tag}.json`), JSON.stringify(parsed, null, 2));
      process.stdout.write(`[collector] result received: ${tag} status=${parsed.status}\n`);
      return sendJson(res, 200, { ok: true, tag });
    }
    if (req.method === "POST" && u.pathname === "/log") {
      const body = await readBody(req, 4 * 1024 * 1024);
      await fsp.appendFile(path.join(LOGS_DIR, "page.log"), body.toString("utf8") + "\n");
      return sendJson(res, 200, { ok: true });
    }
    if (u.pathname === "/health") return sendJson(res, 200, { ok: true });
    if (u.pathname === "/stats") return sendJson(res, 200, stats);
    if (u.pathname === "/reset-stats") {
      stats.files = {};
      stats.startedAt = new Date().toISOString();
      return sendJson(res, 200, stats);
    }
    if (u.pathname === "/results") {
      let raw = "";
      try {
        raw = await fsp.readFile(RAW_RESULTS, "utf8");
      } catch {}
      return sendJson(res, 200, {
        entries: raw
          .split("\n")
          .filter(Boolean)
          .map((l) => JSON.parse(l)),
      });
    }
    if (req.method !== "GET" && req.method !== "HEAD") {
      return sendJson(res, 405, { error: "method not allowed" });
    }
    const abs = resolveFile(u.pathname);
    if (!abs) return sendJson(res, 403, { error: "forbidden" });
    return serveFile(req, res, abs);
  } catch (err) {
    process.stderr.write(`[server] error on ${req.method} ${req.url}: ${err?.stack || err}\n`);
    if (!res.headersSent) sendJson(res, 500, { error: String(err) });
    else res.end();
  }
});

server.listen(PORT, "127.0.0.1", () => {
  process.stdout.write(`[server] listening on http://127.0.0.1:${PORT}/\n`);
  process.stdout.write(`[server] public=${PUBLIC_DIR}\n[server] vendor=${VENDOR_DIR}\n`);
});

for (const sig of ["SIGINT", "SIGTERM"]) {
  process.on(sig, () => {
    process.stdout.write(`[server] ${sig} received, closing\n`);
    server.close(() => process.exit(0));
    setTimeout(() => process.exit(0), 1000).unref();
  });
}
