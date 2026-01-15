const DEFAULT_OWNERS = new Set(["kairyou"]);
const OWNER_CACHE_TTL_MS = 60_000;
const BASES = {
  raw: "https://raw.githubusercontent.com",
  api: "https://api.github.com",
  gist: "https://gist.githubusercontent.com",
  github: "https://github.com"
};
let cachedOwners = null;
let cachedAt = 0;

function extractOwner(kind, parts) {
  if (kind === "raw" || kind === "gist" || kind === "github") {
    return parts[0];
  }
  if (kind === "api") {
    const [root, owner] = parts;
    if (root === "repos" || root === "users" || root === "orgs") {
      return owner;
    }
  }
  return null;
}

function isAllowedPath(kind, parts) {
  if (kind === "raw") {
    return parts.length >= 4; // owner/repo/ref/path
  }
  if (kind === "gist") {
    return parts.length >= 3 && parts[2] === "raw";
  }
  if (kind === "github") {
    if (parts.length < 3) return false; // owner/repo/...
    const rest = parts.slice(2);
    if (rest[0] === "raw") return true;
    if (rest[0] === "archive") return true;
    if (rest[0] === "tarball" || rest[0] === "zipball") return true;
    if (rest[0] === "releases" && rest[1] === "download") return true;
    return false;
  }
  if (kind === "api") {
    return true;
  }
  return false;
}

function parseOwners(value) {
  if (!value) return [];
  if (Array.isArray(value)) return value.map(String);
  if (typeof value === "string") {
    const trimmed = value.trim();
    if (!trimmed) return [];
    if (trimmed.startsWith("[")) {
      try {
        const parsed = JSON.parse(trimmed);
        if (Array.isArray(parsed)) return parsed.map(String);
      } catch {}
    }
    return trimmed.split(/[,\s]+/).filter(Boolean);
  }
  return [];
}

async function loadAllowedOwners(context) {
  const now = Date.now();
  if (cachedOwners && now - cachedAt < OWNER_CACHE_TTL_MS) {
    return cachedOwners;
  }

  const owners = new Set(DEFAULT_OWNERS);
  const kv =
    (context && context.env && context.env.GH_KV) ||
    globalThis.GH_KV;

  if (kv && typeof kv.get === "function") {
    try {
      const raw = await kv.get("allowed_owners");
      for (const owner of parseOwners(raw)) {
        owners.add(owner);
      }
    } catch {}
  }

  cachedOwners = owners;
  cachedAt = now;
  return owners;
}

export default async function onRequest(context) {
  const { request } = context;
  const method = request.method.toUpperCase();
  if (method !== "GET" && method !== "HEAD") {
    return new Response("Method Not Allowed", { status: 405 });
  }

  const url = new URL(request.url);
  const prefix = "/gh";
  if (!url.pathname.startsWith(prefix)) {
    return new Response("Bad Request", { status: 400 });
  }

  let rest = url.pathname.slice(prefix.length);
  if (rest.startsWith("/")) rest = rest.slice(1);
  const parts = rest.split("/").filter(Boolean);
  let kind = parts[0];
  let pathParts = parts.slice(1);
  if (!BASES[kind]) {
    kind = "github";
    pathParts = parts;
  }
  const upstreamBase = BASES[kind];

  const owner = extractOwner(kind, pathParts);
  const allowedOwners = await loadAllowedOwners(context);
  if (!owner || !allowedOwners.has(owner) || !isAllowedPath(kind, pathParts)) {
    return new Response("Forbidden", { status: 403 });
  }

  const upstreamUrl = `${upstreamBase}/${pathParts.join("/")}${url.search}`;
  const headers = new Headers();
  const accept = request.headers.get("accept");
  const range = request.headers.get("range");
  const ifNoneMatch = request.headers.get("if-none-match");
  const ifModifiedSince = request.headers.get("if-modified-since");
  if (accept) headers.set("accept", accept);
  if (range) headers.set("range", range);
  if (ifNoneMatch) headers.set("if-none-match", ifNoneMatch);
  if (ifModifiedSince) headers.set("if-modified-since", ifModifiedSince);
  headers.set("user-agent", "edgeone-gh-proxy");
  if (kind === "api") {
    const token =
      (context && context.env && context.env.GH_TOKEN) ||
      globalThis.GH_TOKEN;
    if (token) {
      headers.set("authorization", `Bearer ${token}`);
    }
  }

  const upstream = await fetch(upstreamUrl, {
    method,
    headers,
    redirect: "follow"
  });

  if (upstream.status >= 400) {
    const body =
      upstream.status === 404 ? "Not Found" : "Upstream Error";
    return new Response(body, {
      status: upstream.status,
      headers: { "content-type": "text/plain;charset=UTF-8" }
    });
  }

  const responseHeaders = new Headers(upstream.headers);
  responseHeaders.set("Access-Control-Allow-Origin", "*");

  return new Response(upstream.body, {
    status: upstream.status,
    headers: responseHeaders
  });
}

/*
Notes (examples):
- raw:
  https://<edgeone>/gh/raw/kairyou/jenkins-cli/main/scripts/install.sh
  https://<edgeone>/gh/raw/kairyou/jenkins-cli/refs/heads/main/scripts/install.sh
- api:
  https://<edgeone>/gh/api/repos/kairyou/jenkins-cli/releases/latest
- gist:
  https://<edgeone>/gh/gist/kairyou/ac3795ad3a19a99fe1201120d5e9b0ff/raw/upstream.sh
- github.com:
  https://<edgeone>/gh/kairyou/jenkins-cli/raw/refs/heads/main/scripts/install.sh
  https://<edgeone>/gh/kairyou/jenkins-cli/releases/download/v0.1.21/jenkins-aarch64-apple-darwin.tar.gz
*/
