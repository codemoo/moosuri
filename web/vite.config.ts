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
        // Package the original raster in a scalable SVG without changing its pixels.
        // Chromium rejects manifest raster sizes above its supported icon bound.
        const raster = readFileSync(resolve(root, "public/moosuri-attendant.png"));
        const icon = `<svg xmlns="http://www.w3.org/2000/svg" width="512" height="512" viewBox="0 0 1254 1254"><image width="1254" height="1254" href="data:image/png;base64,${raster.toString("base64")}"/></svg>`;
        this.emitFile({ type: "asset", fileName: "moosuri-app-icon.svg", source: icon });
        const hash = createHash("sha256");
        hash.update("moosuri-app-icon.svg").update(icon);
        const paths: string[] = ["/moosuri-app-icon.svg"];
        for (const name of Object.keys(bundle).sort()) {
          if (name !== "index.html" && !/^assets\/[\w.-]+\.(?:js|css|woff2)$/.test(name)) continue;
          const item = bundle[name];
          hash.update(name).update(item.type === "chunk" ? item.code : item.source);
          paths.push(`/${name}`);
        }
        for (const name of ["manifest.webmanifest", "icon.svg", "moosuri-attendant.png"]) {
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
  server: { port: 5173, strictPort: true, proxy: { "/api": { target: "http://127.0.0.1:3210", changeOrigin: false } } },
  test: {
    environment: "jsdom",
    setupFiles: ["./src/test-setup.ts"],
    restoreMocks: true,
  },
});
