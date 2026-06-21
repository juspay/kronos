import type {SidebarsConfig} from '@docusaurus/plugin-content-docs';

const sidebars: SidebarsConfig = {
  docsSidebar: [
    {
      type: 'category',
      label: 'Getting Started',
      collapsible: false,
      items: [
        'intro',
        'quickstart',
      ],
    },
    {
      type: 'category',
      label: 'Core Concepts',
      collapsible: true,
      collapsed: false,
      items: [
        'core-concepts/overview',
        'core-concepts/jobs',
        'core-concepts/executions',
        'core-concepts/endpoints',
        'core-concepts/payload-specs',
        'core-concepts/configs',
        'core-concepts/secrets',
        'core-concepts/templates',
        'core-concepts/retry-policy',
        'core-concepts/idempotency',
        'core-concepts/multi-tenancy',
      ],
    },
    {
      type: 'category',
      label: 'Guides',
      collapsible: true,
      collapsed: true,
      items: [
        'guides/http-endpoints',
        'guides/kafka-endpoints',
        'guides/redis-stream-endpoints',
        'guides/cron-jobs',
        'guides/delayed-jobs',
        'guides/versioning',
        'guides/pagination',
        'guides/monitoring',
      ],
    },
    {
      type: 'category',
      label: 'API Reference',
      collapsible: true,
      collapsed: true,
      items: [
        'api/kronos/kronos-task-executor-api',
        {
          type: 'category',
          label: 'Management API',
          items: [
            'api/management/organizations',
            'api/management/workspaces',
            'api/management/configs-api',
            'api/management/secrets-api',
          ],
        },
        {
          type: 'category',
          label: 'Payload Specs',
          items: [
            'api/kronos/create-payload-spec',
            'api/kronos/list-payload-specs',
            'api/kronos/get-payload-spec',
            'api/kronos/update-payload-spec',
            'api/kronos/delete-payload-spec',
          ],
        },
        {
          type: 'category',
          label: 'Endpoints',
          items: [
            'api/kronos/create-endpoint',
            'api/kronos/list-endpoints',
            'api/kronos/get-endpoint',
            'api/kronos/update-endpoint',
            'api/kronos/delete-endpoint',
          ],
        },
        {
          type: 'category',
          label: 'Jobs',
          items: [
            'api/kronos/create-job',
            'api/kronos/list-jobs',
            'api/kronos/get-job',
            'api/kronos/update-job',
            'api/kronos/cancel-job',
            'api/kronos/get-job-status',
            'api/kronos/get-job-versions',
            'api/kronos/list-job-executions',
          ],
        },
        {
          type: 'category',
          label: 'Executions',
          items: [
            'api/kronos/get-execution',
            'api/kronos/cancel-execution',
            'api/kronos/list-execution-attempts',
            'api/kronos/list-execution-logs',
          ],
        },
      ],
    },
    {
      type: 'category',
      label: 'Architecture',
      collapsible: true,
      collapsed: true,
      items: [
        'architecture/overview',
        'architecture/worker-pipeline',
        'architecture/db-driven-scheduling',
        'architecture/exactly-once',
        'architecture/reaper',
        'architecture/dual-deployment',
        'architecture/database-schema',
      ],
    },
    {
      type: 'category',
      label: 'SDKs',
      collapsible: true,
      collapsed: true,
      items: [
        'sdks/overview',
        'sdks/typescript',
        'sdks/rust',
        'sdks/haskell',
      ],
    },
    {
      type: 'category',
      label: 'Deployment',
      collapsible: true,
      collapsed: true,
      items: [
        'deployment/docker',
        'deployment/production',
        'deployment/kms',
        'deployment/dashboard',
      ],
    },
    {
      type: 'category',
      label: 'Configuration',
      collapsible: true,
      collapsed: true,
      items: [
        'configuration/environment-variables',
      ],
    },
    {
      type: 'category',
      label: 'Development',
      collapsible: true,
      collapsed: true,
      items: [
        'development/setup',
        'development/building',
        'development/testing',
        'development/sdk-codegen',
      ],
    },
  ],

  apiSidebar: [
    'api/kronos/kronos-task-executor-api',
    {
      type: 'category',
      label: 'Management API',
      items: [
        'api/management/organizations',
        'api/management/workspaces',
        'api/management/configs-api',
        'api/management/secrets-api',
      ],
    },
    {
      type: 'category',
      label: 'Payload Specs',
      items: [
        'api/kronos/create-payload-spec',
        'api/kronos/list-payload-specs',
        'api/kronos/get-payload-spec',
        'api/kronos/update-payload-spec',
        'api/kronos/delete-payload-spec',
      ],
    },
    {
      type: 'category',
      label: 'Endpoints',
      items: [
        'api/kronos/create-endpoint',
        'api/kronos/list-endpoints',
        'api/kronos/get-endpoint',
        'api/kronos/update-endpoint',
        'api/kronos/delete-endpoint',
      ],
    },
    {
      type: 'category',
      label: 'Jobs',
      items: [
        'api/kronos/create-job',
        'api/kronos/list-jobs',
        'api/kronos/get-job',
        'api/kronos/update-job',
        'api/kronos/cancel-job',
        'api/kronos/get-job-status',
        'api/kronos/get-job-versions',
        'api/kronos/list-job-executions',
      ],
    },
    {
      type: 'category',
      label: 'Executions',
      items: [
        'api/kronos/get-execution',
        'api/kronos/cancel-execution',
        'api/kronos/list-execution-attempts',
        'api/kronos/list-execution-logs',
      ],
    },
  ],
};

export default sidebars;
