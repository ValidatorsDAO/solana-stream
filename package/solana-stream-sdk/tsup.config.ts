import { defineConfig } from 'tsup'

export default defineConfig({
  entry: ['src/index.ts', 'src/geyser-client.ts'],
  format: ['esm'],
  dts: true,
  sourcemap: true,
  clean: true,
  splitting: false,
  shims: true,
  esbuildOptions(options) {
    options.platform = 'node'
    options.format = 'esm'
    options.loader = { '.js': 'jsx' }
  },
})
