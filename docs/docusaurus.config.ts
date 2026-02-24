import { themes as prismThemes } from "prism-react-renderer";
import type { Config } from "@docusaurus/types";
import type * as Preset from "@docusaurus/preset-classic";
import searchLocal from "@easyops-cn/docusaurus-search-local";

const docsVersion = process.env.DOCS_VERSION || "latest";
const baseUrl = process.env.DOCS_BASE_URL || "/";
const isLatest = docsVersion === "latest";

const config: Config = {
  title: "RepX",
  tagline: "Reproducible HPC Experiments Framework",
  favicon: "img/logo.svg",

  url: "https://repx-org.github.io",
  baseUrl: baseUrl,

  organizationName: "repx-org",
  projectName: "repx",

  onBrokenLinks: "warn",

  i18n: {
    defaultLocale: "en",
    locales: ["en"],
  },

  customFields: {
    docsVersion: docsVersion,
    isLatest: isLatest,
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
              label: docsVersion,
            },
          },
        },
        theme: {
          customCss: "./src/css/custom.css",
        },
      } satisfies Preset.Options,
    ],
  ],

  themes: [
    [
      searchLocal,
      {
        hashed: true,
      },
    ],
  ],

  themeConfig: {
    ...((!isLatest)
      ? {
          announcementBar: {
            id: "version_notice",
            content: `You are viewing docs for <strong>${docsVersion}</strong>. <a href="/latest/docs/getting-started/quickstart">See latest version</a>.`,
            backgroundColor: "#fff3cd",
            textColor: "#856404",
            isCloseable: true,
          },
        }
      : {}),
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
          type: "custom-versionSelector",
          position: "right",
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
      copyright: `Copyright \u00a9 ${new Date().getFullYear()} RepX Organization.`,
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
