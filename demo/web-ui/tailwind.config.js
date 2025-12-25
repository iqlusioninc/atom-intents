/** @type {import('tailwindcss').Config} */
export default {
  content: [
    "./index.html",
    "./src/**/*.{js,ts,jsx,tsx}",
  ],
  theme: {
    extend: {
      fontFamily: {
        sans: ['Inter', 'system-ui', '-apple-system', 'sans-serif'],
        display: ['Inter', 'system-ui', '-apple-system', 'sans-serif'],
      },
      colors: {
        // Cosmos Network inspired palette
        cosmos: {
          // Primary purple - the signature Cosmos color
          50: '#f5f3ff',
          100: '#ede9fe',
          200: '#ddd6fe',
          300: '#c4b5fd',
          400: '#a78bfa',
          500: '#8b5cf6',
          600: '#7c3aed',
          700: '#6d28d9',
          800: '#5b21b6',
          900: '#4c1d95',
          950: '#2e1065',
        },
        // Deep space backgrounds
        space: {
          50: '#f8fafc',
          100: '#f1f5f9',
          200: '#e2e8f0',
          300: '#cbd5e1',
          400: '#94a3b8',
          500: '#64748b',
          600: '#475569',
          700: '#334155',
          800: '#1e293b',
          850: '#172033',
          900: '#0f172a',
          925: '#0c1322',
          950: '#080d19',
        },
        // Accent colors for Cosmos ecosystem
        atom: {
          purple: '#6f7390',
          blue: '#2d54dd',
          cyan: '#00d1ff',
          green: '#22c55e',
          gold: '#fbbf24',
        },
      },
      backgroundImage: {
        // Cosmos-style gradients
        'cosmos-gradient': 'linear-gradient(135deg, #0f172a 0%, #1e1854 50%, #0f172a 100%)',
        'cosmos-radial': 'radial-gradient(ellipse at top, #1e1854 0%, #0f172a 50%, #080d19 100%)',
        'glow-purple': 'radial-gradient(ellipse at center, rgba(139, 92, 246, 0.15) 0%, transparent 70%)',
        'glow-cyan': 'radial-gradient(ellipse at center, rgba(0, 209, 255, 0.1) 0%, transparent 70%)',
        'card-gradient': 'linear-gradient(180deg, rgba(255,255,255,0.05) 0%, rgba(255,255,255,0.02) 100%)',
        'mesh-gradient': `
          radial-gradient(at 40% 20%, rgba(139, 92, 246, 0.15) 0px, transparent 50%),
          radial-gradient(at 80% 0%, rgba(45, 84, 221, 0.1) 0px, transparent 50%),
          radial-gradient(at 0% 50%, rgba(139, 92, 246, 0.1) 0px, transparent 50%),
          radial-gradient(at 80% 50%, rgba(0, 209, 255, 0.05) 0px, transparent 50%),
          radial-gradient(at 0% 100%, rgba(139, 92, 246, 0.1) 0px, transparent 50%)
        `,
      },
      boxShadow: {
        'glow': '0 0 20px rgba(139, 92, 246, 0.3)',
        'glow-sm': '0 0 10px rgba(139, 92, 246, 0.2)',
        'glow-lg': '0 0 40px rgba(139, 92, 246, 0.4)',
        'glow-cyan': '0 0 20px rgba(0, 209, 255, 0.3)',
        'inner-glow': 'inset 0 1px 0 rgba(255, 255, 255, 0.1)',
        'card': '0 4px 6px -1px rgba(0, 0, 0, 0.2), 0 2px 4px -2px rgba(0, 0, 0, 0.1)',
      },
      animation: {
        'pulse-slow': 'pulse 3s cubic-bezier(0.4, 0, 0.6, 1) infinite',
        'glow': 'glow 2s ease-in-out infinite alternate',
        'float': 'float 6s ease-in-out infinite',
        'gradient': 'gradient 8s ease infinite',
      },
      keyframes: {
        glow: {
          '0%': { opacity: 0.5 },
          '100%': { opacity: 1 },
        },
        float: {
          '0%, 100%': { transform: 'translateY(0px)' },
          '50%': { transform: 'translateY(-10px)' },
        },
        gradient: {
          '0%, 100%': { backgroundPosition: '0% 50%' },
          '50%': { backgroundPosition: '100% 50%' },
        },
      },
      borderRadius: {
        '4xl': '2rem',
      },
    },
  },
  plugins: [],
}
