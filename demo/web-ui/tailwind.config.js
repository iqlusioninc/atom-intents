/** @type {import('tailwindcss').Config} */
export default {
  content: [
    "./index.html",
    "./src/**/*.{js,ts,jsx,tsx}",
  ],
  theme: {
    extend: {
      colors: {
        cosmos: {
          50: '#f0f4ff',
          100: '#e0e8ff',
          200: '#c7d4ff',
          300: '#a3b5ff',
          400: '#7a8bff',
          500: '#5a5fff',
          600: '#4d3df5',
          700: '#4130d8',
          800: '#362aae',
          900: '#302889',
          950: '#1e1854',
        },
      },
    },
  },
  plugins: [],
}
