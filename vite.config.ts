import { defineConfig } from 'vite';
import react from '@vitejs/plugin-react';

export default defineConfig({
  plugins: [react()],
  clearScreen: false,
  server: {
    host: '127.0.0.1',
    // Windows 可能把 1024–5000 中的一段划给 Hyper-V/WinNAT 预留端口，
    // 落在预留区间时监听会直接报 EACCES。这里改用 Vite 常用端口，
    // 同时与 tauri.conf.json 的 devUrl、devCsp 保持一致。
    port: 5173,
    strictPort: true,
    hmr: {
      host: '127.0.0.1',
      port: 5174,
    },
    watch: {
      ignored: ['**/src-tauri/**'],
    },
  },
  build: {
    target: ['es2022', 'safari15'],
    chunkSizeWarningLimit: 1000,
    rolldownOptions: {
      output: {
        codeSplitting: {
          groups: [
            {
              name: 'vendor',
              test: /node_modules/,
            },
          ],
        },
      },
    },
  },
});
