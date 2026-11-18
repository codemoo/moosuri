import { defineConfig } from "vitest/config";
import react from "@vitejs/plugin-react";
import { createHash } from "node:crypto";
import { readFileSync } from "node:fs";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { build } from "vite";
const root = dirname(fileURLToPath(import.meta.url));
export default defineConfig({
  plugins: [react(), {
    name: "moosuri-public-shell",
    apply: "build",
    enforce: "post",
    generateBundle: {
      order: "post",
      async handler(_options, bundle) {
        const hash = createHash("sha256");
        const paths: string[] = [];
        for (const name of Object.keys(bundle).sort()) {
          if (name !== "index.html" && !/^assets\/[\w.-]+\.(?:js|css|woff2)$/.test(name)) continue;
          const item = bundle[name];
          hash.update(name).update(item.type === "chunk" ? item.code : item.source);
          paths.push(`/${name}`);
        }
        for (const name of ["manifest.webmanifest", "moosuri-icon-192.png", "moosuri-icon-512.png"]) {
          hash.update(name).update(readFileSync(resolve(root, "public", name)));
          paths.push(`/${name}`);
        }
        if (!paths.includes("/index.html")) throw new Error("Public shell is missing index.html");
        // Real module bundling: worker dependencies share no window/React runtime.
        const result = await build({
          configFile: false, root, publicDir: false, logLevel: "error",
          build: { write: false, emptyOutDir: false, minify: false, target: "es2022",
            lib: { entry: resolve(root, "service-worker.ts"), formats: ["iife"], name: "MoosuriWorker" },
          },
        });
        const outputs = Array.isArray(result) ? result : [result];
        const chunks = outputs.flatMap(output => "output" in output ? output.output : []);
        if (chunks.length !== 1 || chunks[0].type !== "chunk") throw new Error("Worker must be one bundled script");
        const compiled = chunks[0].code;
        hash.update(compiled);
        const prefix = `const MOOSURI_SHELL_VERSION=${JSON.stringify(hash.digest("hex").slice(0, 24))};\nconst MOOSURI_SHELL_ASSETS=${JSON.stringify(paths)};\n`;
        this.emitFile({ type: "asset", fileName: "service-worker.js", source: prefix + compiled });
      },
    },
  }],
  build: {
    rollupOptions: {
      input: { main: resolve(root, "index.html"), personal: resolve(root, "personal.html") },
    },
  },
  server: { port: 5173, strictPort: true, proxy: { "/api": { target: "http://127.0.0.1:3210", changeOrigin: false } } },
  test: {
    environment: "jsdom",
    setupFiles: ["./src/test-setup.ts"],
    restoreMocks: true,
  },
});
