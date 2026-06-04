import { defineConfig } from 'vite';
import react from '@vitejs/plugin-react';
import path from 'node:path';

const host = process.env.TAURI_DEV_HOST;

// Vite + Tauri 2: 1420 is Tauri default; HMR over ws://1421 for mobile, unused on desktop.
export default defineConfig({
  plugins: [react()],
  resolve: {
    alias: {
      '@': path.resolve(__dirname, './src'),
    },
  },
  clearScreen: false,
  server: {
    port: 1420,
    strictPort: true,
    host: host ?? false,
    hmr: host ? { protocol: 'ws', host, port: 1421 } : undefined,
    watch: {
      ignored: ['**/src-tauri/**'],
    },
  },
  envPrefix: ['VITE_', 'TAURI_ENV_'],
  build: {
    target: 'es2022',
    minify: 'esbuild',
    sourcemap: true,
    chunkSizeWarningLimit: 800,
    rollupOptions: {
      output: {
        // Group stable third-party deps into long-cache-friendly vendor
        // chunks; everything else stays in route/dynamic-import chunks. The
        // codemirror vendor stays attached to the editor's lazy chunk so it
        // doesn't bloat the entry — we route via the module path.
        manualChunks(id) {
          if (!id.includes('node_modules')) return undefined;
          if (id.includes('/@codemirror/') || id.match(/\/node_modules\/codemirror\//)) {
            return 'vendor-codemirror';
          }
          if (
            id.includes('/react-dom/') ||
            id.match(/\/node_modules\/react\//) ||
            id.includes('/react-i18next/') ||
            id.includes('/i18next/') ||
            id.includes('/scheduler/')
          ) {
            return 'vendor-react';
          }
          if (
            id.includes('/@radix-ui/') ||
            id.includes('/cmdk/') ||
            id.includes('/lucide-react/')
          ) {
            return 'vendor-radix';
          }
          return undefined;
        },
      },
    },
  },
});
