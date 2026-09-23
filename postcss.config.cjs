// The local-first build chain (the runtime CDN is gone): Tailwind v3 JIT +
// autoprefixer, driven by Vite's automatic PostCSS pass over src/app.css.
module.exports = {
  plugins: {
    tailwindcss: {},
    autoprefixer: {},
  },
};
