/** @type {import('tailwindcss').Config} */
export default {
  content: ["./index.html", "./src/**/*.{ts,tsx}"],
  theme: {
    extend: {
      fontFamily: {
        sans: [
          "-apple-system",
          "BlinkMacSystemFont",
          "PingFang SC",
          "Hiragino Sans GB",
          "Microsoft YaHei",
          "sans-serif",
        ],
      },
      colors: {
        brand: {
          DEFAULT: "#1a56db",
          dark: "#1040b8",
          soft: "#e8f0fe",
        },
      },
    },
  },
  plugins: [],
};
