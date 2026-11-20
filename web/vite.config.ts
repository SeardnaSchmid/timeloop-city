import { defineConfig } from 'vite';
import react from '@vitejs/plugin-react';

// Dev: `npm run dev` serves the UI on :5173 and forwards /api to the
// running simulation (`sim serve`, :7878). Build: `dist/`, which
// `sim serve --web web/dist` serves itself.
export default defineConfig({
  plugins: [react()],
  server: { port: 5173, proxy: { '/api': 'http://127.0.0.1:7878' } },
});
