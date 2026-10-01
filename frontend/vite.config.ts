// frontend/vite.config.ts
import { sveltekit } from '@sveltejs/kit/vite';
import tailwindcss from '@tailwindcss/vite';
import { SvelteKitPWA } from '@vite-pwa/sveltekit';
import { defineConfig } from 'vite';

export default defineConfig({
    plugins: [
        sveltekit(),
        tailwindcss(),
        SvelteKitPWA({
            srcDir: './src',
            mode: 'production',
            strategies: 'generateSW',
            registerType: 'autoUpdate',
            manifest: {
                name: 'Photo App',
                short_name: 'Photos',
                description: 'Self-hosted AI-powered media vault',
                theme_color: '#090a0f',
                background_color: '#090a0f',
                display: 'standalone',
                orientation: 'portrait-primary',
                scope: '/',
                start_url: '/',
                icons: [
                    {
                        src: '/logo-192.png',
                        sizes: '192x192',
                        type: 'image/png'
                    },
                    {
                        src: '/logo-512.png',
                        sizes: '512x512',
                        type: 'image/png'
                    },
                    {
                        src: '/logo-512.png',
                        sizes: '512x512',
                        type: 'image/png',
                        purpose: 'any maskable'
                    }
                ]
            },
            workbox: {
                globPatterns: ['client/**/*.{js,css,ico,png,svg,webp,woff,woff2}'],
                // Never let the service worker intercept live SSE streams or raw media files
                navigateFallbackDenylist: [/^\/api\//, /^\/users\//],
                runtimeCaching: [
                    {
                        // Cache thumbnails with StaleWhileRevalidate for 3 days only
                        urlPattern: /^.*\/thumbs\/.*\.(webp|jpg|jpeg|png)$/,
                        handler: 'StaleWhileRevalidate',
                        options: {
                            cacheName: 'photo-thumbnails-cache',
                            expiration: {
                                maxEntries: 300,
                                maxAgeSeconds: 60 * 60 * 24 * 3 // 3 days
                            },
                            cacheableResponse: {
                                statuses: [0, 200]
                            }
                        }
                    }
                ]
            },
            devOptions: {
                enabled: true,
                type: 'module'
            }
        })
    ],
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
                changeOrigin: true,
                timeout: 0,       // 0 disables the proxy timeout
                proxyTimeout: 0,  // 0 disables upstream response timeout
            },
            '/users': {
                target: 'http://127.0.0.1:3000',
                changeOrigin: true
            }
        }
    }
});