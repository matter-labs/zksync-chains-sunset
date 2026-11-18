import type { Config } from "tailwindcss";

export default {
  content: ["./src/**/*.{astro,html,js,jsx,ts,tsx}"],
  theme: {
    extend: {
      colors: {
        bg: "var(--bg)",
        panel: "var(--panel-bg)",
        stroke: "var(--stroke)",
        text: "var(--text)",
        muted: "var(--muted)",
        primary: "var(--primary)",
        white: "var(--white)",
      },
      borderRadius: {
        lg: "var(--radius-lg)",
        pill: "var(--radius-pill)",
      },
      spacing: {
        pad: "var(--pad)",
      },
      maxWidth: {
        container: "var(--container)",
      },
      fontFamily: {
        sans: ["var(--font-sans)"],
      },
    },
  },
  plugins: [],
} satisfies Config;
