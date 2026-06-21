import clsx from 'clsx';
import useDocusaurusContext from '@docusaurus/useDocusaurusContext';
import Link from '@docusaurus/Link';
import Heading from '@theme/Heading';
import Layout from '@theme/Layout';

import styles from './index.module.css';

function HeroHeader() {
  const {siteConfig} = useDocusaurusContext();
  return (
    <header className={clsx('hero hero--primary', styles.heroBanner)}>
      <div className="container">
        <Heading as="h1" className="hero__title">
          {siteConfig.title}
        </Heading>
        <p className="hero__subtitle">
          Durable job scheduling &mdash; setTimeout and setInterval as a service.
        </p>
        <p className={styles.heroTagline}>
          Distributed, durable, retriable, observable delivery of jobs to HTTP endpoints,
          Kafka topics, and Redis Streams &mdash; with type-safety guarantees.
        </p>
        <div className={styles.buttons}>
          <Link
            className="button button--secondary button--lg"
            to="/docs/intro">
            Get Started
          </Link>
          <Link
            className="button button--primary button--lg"
            to="/docs/quickstart">
            Quickstart
          </Link>
        </div>
      </div>
    </header>
  );
}

const features = [
  {
    title: 'Exactly-Once Delivery',
    description: (
      <>
        Idempotency keys, DB unique constraints, and <code>SELECT FOR UPDATE SKIP LOCKED</code>
        ensure jobs never fire twice &mdash; even across crashes and retries.
      </>
    ),
  },
  {
    title: 'Multi-Tenant',
    description: (
      <>
        Schema-per-workspace isolation. Each tenant gets its own PostgreSQL schema
        with fully isolated tables. Shared-nothing between tenants.
      </>
    ),
  },
  {
    title: 'Sub-Second Latency',
    description: (
      <>
        Immediate jobs fire in ~300ms. Delayed jobs execute within ~200ms of their
        scheduled time. No separate scheduler process needed.
      </>
    ),
  },
  {
    title: 'Fully Observable',
    description: (
      <>
        Every execution has a lifecycle. Every attempt recorded with duration, output,
        and error. Prometheus metrics and Grafana dashboards included.
      </>
    ),
  },
  {
    title: 'Type-Safe',
    description: (
      <>
        JSON Schema validation on job input at creation time. Catch invalid payloads
        before they reach your endpoints. Templates resolved at execution time.
      </>
    ),
  },
  {
    title: 'Pluggable Dispatchers',
    description: (
      <>
        Built-in support for HTTP, Kafka, and Redis Streams. Add new endpoint types
        via feature flags. Same retry policy, same guarantees, regardless of transport.
      </>
    ),
  },
];

function Feature({title, description}: {title: string; description: React.ReactNode}) {
  return (
    <div className={clsx('col col--4')}>
      <div className="text--center padding-horiz--md">
        <Heading as="h3">{title}</Heading>
        <p>{description}</p>
      </div>
    </div>
  );
}

function Features() {
  return (
    <section className={styles.features}>
      <div className="container">
        <div className="row">
          {features.map((props, idx) => (
            <Feature key={idx} {...props} />
          ))}
        </div>
      </div>
    </section>
  );
}

const mentalModel = [
  { js: 'setTimeout(fn, 0)', kronos: 'POST /v1/jobs { trigger: IMMEDIATE }', desc: 'Fire now' },
  { js: 'setTimeout(fn, 5000)', kronos: 'POST /v1/jobs { trigger: DELAYED, run_at: "..." }', desc: 'Fire later' },
  { js: 'setInterval(fn, 60000)', kronos: 'POST /v1/jobs { trigger: CRON, cron: "* * * * *" }', desc: 'Fire repeatedly' },
  { js: 'clearTimeout(id)', kronos: 'POST /v1/jobs/{id}/cancel', desc: 'Cancel' },
];

function MentalModel() {
  return (
    <section className={styles.mentalModel}>
      <div className="container">
        <Heading as="h2" className="text--center margin-bottom--lg">
          If you know JavaScript, you know Kronos
        </Heading>
        <div className="row">
          {mentalModel.map((item, idx) => (
            <div key={idx} className={clsx('col col--3')}>
              <div className={styles.modelCard}>
                <p className={styles.modelDesc}>{item.desc}</p>
                <p className={styles.modelJS}>{item.js}</p>
                <p className={styles.modelArrow}>&darr;</p>
                <p className={styles.modelKronos}>{item.kronos}</p>
              </div>
            </div>
          ))}
        </div>
        <p className="text--center margin-top--lg">
          Except: it survives crashes, retries on failure, never fires twice, and every execution is observable.
        </p>
      </div>
    </section>
  );
}

export default function Home(): React.ReactNode {
  return (
    <Layout>
      <HeroHeader />
      <main>
        <Features />
        <MentalModel />
      </main>
    </Layout>
  );
}
