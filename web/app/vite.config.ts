import { defineConfig } from "vite";
export default defineConfig({
  server: { proxy: { "/split": "http://127.0.0.1:8080", "/openapi.json": "http://127.0.0.1:8080" } },
});
