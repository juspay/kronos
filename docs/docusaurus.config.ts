import {themes as prismThemes} from 'prism-react-renderer';
import type * as Preset from '@docusaurus/preset-classic';
import type {Config} from '@docusaurus/types';
import type * as OpenApiPlugin from 'docusaurus-plugin-openapi-docs';

const config: Config = {
  title: 'Kronos',
  tagline: 'Durable job scheduling — setTimeout and setInterval as a service',
  favicon: 'img/favicon.ico',

  url: 'https://kronos.example.com',
  baseUrl: '/',

  organizationName: 'juspay',
  projectName: 'kronos',

  onBrokenLinks: 'throw',

  presets: [
    [
      'classic',
      {
        docs: {
          path: 'docs',
          routeBasePath: '/docs',
          sidebarPath: './sidebars.ts',
          docItemComponent: '@theme/ApiItem',
          exclude: ['superpowers/**'],
        },
        blog: false,
        theme: {
          customCss: './src/css/custom.css',
        },
      } satisfies Preset.Options,
    ],
  ],

  plugins: [
    [
      'docusaurus-plugin-openapi-docs',
      {
        id: 'api',
        docsPluginId: 'classic',
        config: {
          kronos: {
            specPath: 'api/kronos-openapi.json',
            outputDir: 'docs/api/kronos',
            sidebarOptions: {
              groupPathsBy: 'tag',
            },
          } satisfies OpenApiPlugin.Options,
        },
      },
    ],
  ],

  themes: ['docusaurus-theme-openapi-docs'],

  themeConfig: {
    colorMode: {
      defaultMode: 'dark',
      disableSwitch: false,
      respectPrefersColorScheme: true,
    },
    navbar: {
      title: 'Kronos',
      logo: {
        alt: 'Kronos Logo',
        src: 'img/logo.svg',
      },
      items: [
        {
          type: 'docSidebar',
          sidebarId: 'docsSidebar',
          position: 'left',
          label: 'Docs',
        },
        {
          type: 'docSidebar',
          sidebarId: 'apiSidebar',
          position: 'left',
          label: 'API',
        },
        {
          href: 'https://github.com/juspay/kronos',
          label: 'GitHub',
          position: 'right',
        },
      ],
    },
    footer: {
      style: 'dark',
      links: [
        {
          title: 'Docs',
          items: [
            {
              label: 'Introduction',
              to: '/docs/intro',
            },
            {
              label: 'Quickstart',
              to: '/docs/quickstart',
            },
            {
              label: 'API Reference',
              to: '/docs/api/kronos/kronos-task-executor-api',
            },
          ],
        },
        {
          title: 'Community',
          items: [
            {
              label: 'GitHub',
              href: 'https://github.com/juspay/kronos',
            },
          ],
        },
        {
          title: 'More',
          items: [
            {
              label: 'Architecture',
              to: '/docs/architecture/overview',
            },
            {
              label: 'Deployment',
              to: '/docs/deployment/docker',
            },
          ],
        },
      ],
      copyright: `Copyright ${new Date().getFullYear()} Kronos. Built with Docusaurus.`,
    },
    prism: {
      theme: prismThemes.github,
      darkTheme: prismThemes.dracula,
      additionalLanguages: ['bash', 'json', 'typescript', 'rust', 'python', 'yaml'],
    },
    api: {
      proxy: 'http://localhost:8080',
    },
  } satisfies Preset.ThemeConfig,
};

export default config;
