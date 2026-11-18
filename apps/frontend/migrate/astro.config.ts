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
    port: 4322,
  },
  vite: {
    define: {
      "import.meta.env.PUBLIC_WALLETCONNECT_DISABLED": JSON.stringify("true"),
      "import.meta.env.PUBLIC_WALLETCONNECT_PROJECT_ID": JSON.stringify(""),
    },
    resolve: {
      alias: {
        "@shared": path.resolve(here, "../shared/src"),
      },
    },
    server: {
      proxy: {
        "/api": "http://localhost:3000",
      },
    },
  },
});
