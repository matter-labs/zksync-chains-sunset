import react from "@astrojs/react";
import tailwind from "@astrojs/tailwind";
import { defineConfig } from "astro/config";
import path from "node:path";
import { fileURLToPath } from "node:url";

const here = path.dirname(fileURLToPath(import.meta.url));

export default defineConfig({
  output: "static",
  publicDir: path.resolve(here, "../shared/public"),
  integrations: [tailwind(), react()],
  server: {
    host: true,
    port: 4321,
  },
  vite: {
    resolve: {
      alias: {
        "@shared": path.resolve(here, "../shared/src"),
      },
    },
    server: {
      proxy: {
        "/api": "http://localhost:3000",
        "/readyz": "http://localhost:3000",
      },
    },
  },
});
