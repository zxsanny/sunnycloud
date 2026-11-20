import { vitePreprocess } from '@sveltejs/vite-plugin-svelte';

export default {
  preprocess: vitePreprocess(),
  vitePlugin: {
    // Runes mode for our own components only; libraries from node_modules keep what they were built with.
    dynamicCompileOptions({ filename }) {
      return filename.includes('node_modules') ? undefined : { runes: true };
    },
  },
};
