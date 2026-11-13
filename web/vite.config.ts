import { defineConfig } from "vitest/config";
import react from "@vitejs/plugin-react";
import { createHash } from "node:crypto";
import { readFileSync } from "node:fs";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import ts from "typescript";
const root = dirname(fileURLToPath(import.meta.url));
export default defineConfig({
  plugins: [react(), {
    name: "moosuri-public-shell",
    apply: "build",
    enforce: "post",
    generateBundle: {
      order: "post",
      handler(_options, bundle) {
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
        const worker = readFileSync(resolve(root, "service-worker.ts"), "utf8");
        hash.update(worker);
        const prefix = `const MOOSURI_SHELL_VERSION=${JSON.stringify(hash.digest("hex").slice(0, 24))};\nconst MOOSURI_SHELL_ASSETS=${JSON.stringify(paths)};\n`;
        const compiled = ts.transpileModule(worker, {
          compilerOptions: { target: ts.ScriptTarget.ES2022, module: ts.ModuleKind.ESNext },
        }).outputText.replace(/^export \{\};?\s*$/m, "");
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
