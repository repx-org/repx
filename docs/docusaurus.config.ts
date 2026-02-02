import { themes as prismThemes } from "prism-react-renderer";
import type { Config } from "@docusaurus/types";
import type * as Preset from "@docusaurus/preset-classic";

const config: Config = {
  title: "RepX",
  tagline: "Reproducible HPC Experiments Framework",
  favicon: "img/logo.svg",

  url: "https://repx-org.github.io",
  baseUrl: "/",

  organizationName: "repx-org",
  projectName: "repx",

  onBrokenLinks: "warn",

  i18n: {
    defaultLocale: "en",
    locales: ["en"],
  },

  presets: [
    [
      "classic",
      {
        docs: {
          path: "docs",
          sidebarPath: "./sidebars.ts",
          versions: {
            current: {
              label: "latest",
            },
          },
        },
        theme: {
          customCss: "./src/css/custom.css",
        },
      } satisfies Preset.Options,
    ],
  ],

  themeConfig: {
    navbar: {
      title: "RepX",
      logo: {
        alt: "RepX Logo",
        src: "img/logo.svg",
      },
      items: [
        {
          type: "docSidebar",
          sidebarId: "docsSidebar",
          position: "left",
          label: "Docs",
        },
        {
          href: "https://github.com/repx-org/repx",
          label: "GitHub",
          position: "right",
        },
      ],
    },
    footer: {
      style: "dark",
      links: [
        {
          title: "Resources",
          items: [
            {
              label: "Docs",
              to: "/",
            },
          ],
        },
        {
          title: "More",
          items: [
            {
              label: "GitHub",
              href: "https://github.com/repx-org/repx",
            },
          ],
        },
      ],
      copyright: `Copyright © ${new Date().getFullYear()} RepX Organization.`,
    },
    prism: {
      theme: prismThemes.github,
      darkTheme: prismThemes.dracula,
      additionalLanguages: ["bash", "diff", "nix", "python"],
    },
  } satisfies Preset.ThemeConfig,

  markdown: {
    format: "mdx",
  },
};

export default config;
