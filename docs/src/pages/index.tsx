import clsx from 'clsx';
import Link from '@docusaurus/Link';
import useDocusaurusContext from '@docusaurus/useDocusaurusContext';
import Layout from '@theme/Layout';
import Heading from '@theme/Heading';

import styles from './index.module.css';

function HomepageHeader() {
  const {siteConfig} = useDocusaurusContext();
  return (
    <header className={clsx('hero hero--primary', styles.heroBanner)}>
      <div className="container">
        <Heading as="h1" className="hero__title">
          {siteConfig.title}
        </Heading>
        <p className="hero__subtitle">{siteConfig.tagline}</p>
        <div className={styles.buttons}>
          <Link
            className="button button--secondary button--lg"
            to="/docs/getting-started/quickstart">
            Get Started 🚀
          </Link>
        </div>
      </div>
    </header>
  );
}

export default function Home(): JSX.Element {
  const {siteConfig} = useDocusaurusContext();
  return (
    <Layout
      title={`${siteConfig.title}`}
      description="Reproducible HPC Experiments Framework">
      <HomepageHeader />
      <main>
        <div className="container">
          <section className="row margin-vert--xl">
            <div className={clsx('col col--4')}>
              <div className="text--center">
                <Heading as="h3">Define</Heading>
                <p>Declarative Nix DSL for stages, pipelines, and parameter sweeps.</p>
              </div>
            </div>
            <div className={clsx('col col--4')}>
              <div className="text--center">
                <Heading as="h3">Run</Heading>
                <p>Local or HPC clusters via SSH/SLURM. Container isolation ensures consistent environments.</p>
              </div>
            </div>
            <div className={clsx('col col--4')}>
              <div className="text--center">
                <Heading as="h3">Analyze</Heading>
                <p>Python API for querying results and metadata from the structured output store.</p>
              </div>
            </div>
          </section>
        </div>
      </main>
    </Layout>
  );
}
