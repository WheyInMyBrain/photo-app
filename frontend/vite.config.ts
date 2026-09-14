// frontend/vite.config.ts
import { sveltekit } from '@sveltejs/kit/vite';
import tailwindcss from '@tailwindcss/vite';
import { defineConfig } from 'vite';

export default defineConfig({
    plugins: [sveltekit(), tailwindcss()],
    server: {
        port: 5173,
        proxy: {
            '/api/events': {
                target: 'http://127.0.0.1:3000',
                changeOrigin: true,
                configure: (proxy) => {
                    proxy.on('proxyRes', (proxyRes) => {
                        proxyRes.headers['cache-control'] = 'no-cache, no-transform';
                        proxyRes.headers['x-accel-buffering'] = 'no';
                    });
                }
            },
            '/api': {
                target: 'http://127.0.0.1:3000',
                changeOrigin: true
            },
            '/users': {
                target: 'http://127.0.0.1:3000',
                changeOrigin: true
            }
        }
    }
});